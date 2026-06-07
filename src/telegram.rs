use std::net::SocketAddr;

use crate::agent_runner::run_agent_turn;
use crate::config::AppConfig;
use axum::extract::Json;
use axum::routing::post;
use axum::Router;
use serde::Deserialize;
use tokio::net::TcpListener;

use crate::intake::{build_incoming_query, prepare_turn_request, IncomingQueryParts, IntakeError};
use crate::openai_responses::OpenAiResponsesHttpClient;
use crate::session::derive_telegram_session_key;
use crate::thinkingroot_boundary::preview_before_thinkingroot;
use crate::thinkingroot_sdk::ThinkingRootSdkClient;
use crate::types::{AgentTurnRequest, AgentTurnResult, PreparedTurnPreview, SourceType};

#[derive(Debug, Deserialize)]
pub struct TelegramUpdate {
    pub update_id: i64,
    pub message: Option<TelegramMessage>,
}

#[derive(Debug, Deserialize)]
pub struct TelegramMessage {
    pub message_id: i64,
    pub from: Option<TelegramUser>,
    pub chat: TelegramChat,
    pub text: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TelegramUser {
    pub id: i64,
    pub username: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TelegramChat {
    pub id: i64,
    #[serde(rename = "type")]
    pub chat_type: Option<String>,
    pub title: Option<String>,
    pub username: Option<String>,
}

pub fn prepare_from_update(
    update: TelegramUpdate,
    workspace_hint: Option<String>,
) -> Result<PreparedTurnPreview, IntakeError> {
    let message = update
        .message
        .ok_or(IntakeError::UnsupportedTelegramUpdate)?;
    let text = message.text.ok_or(IntakeError::UnsupportedTelegramUpdate)?;

    let chat_id = message.chat.id.to_string();
    let user_id = message.from.as_ref().map(|user| user.id.to_string());
    let session_key = derive_telegram_session_key(&chat_id, user_id.as_deref());

    let incoming = build_incoming_query(IncomingQueryParts {
        source_type: SourceType::Telegram,
        raw_text: text,
        session_key,
        user_id,
        chat_id: Some(chat_id),
        message_id: Some(message.message_id.to_string()),
        cwd: None,
        workspace_hint,
    })?;

    Ok(preview_before_thinkingroot(prepare_turn_request(incoming)))
}

pub async fn serve_telegram(
    bind: SocketAddr,
    workspace_hint: Option<String>,
) -> anyhow_free::Result<()> {
    let app = Router::new().route(
        "/telegram/webhook",
        post(move |Json(update): Json<TelegramUpdate>| {
            let workspace_hint = workspace_hint.clone();
            async move {
                match prepare_from_update(update, workspace_hint) {
                    Ok(preview) => {
                        let request = &preview.prepared_turn_request;
                        tracing::info!(
                            request_id = %request.request_id,
                            session_key = %request.session_key,
                            query_text = %request.query_text,
                            "prepared Telegram query before ThinkingRoot"
                        );
                        println!(
                            "[telegram] request_id={} session_key={} query=\"{}\" boundary=\"{}\"",
                            request.request_id,
                            request.session_key,
                            request.query_text,
                            preview.thinkingroot_boundary
                        );
                        Ok(Json(preview))
                    }
                    Err(error) => Err((axum::http::StatusCode::BAD_REQUEST, error.to_string())),
                }
            }
        }),
    );

    let listener = TcpListener::bind(bind).await?;
    tracing::info!(%bind, "serving Telegram webhook intake");
    axum::serve(listener, app).await?;
    Ok(())
}

pub async fn serve_telegram_run(
    bind: SocketAddr,
    workspace_hint: Option<String>,
) -> anyhow_free::Result<()> {
    let app = Router::new().route(
        "/telegram/webhook",
        post(move |Json(update): Json<TelegramUpdate>| {
            let workspace_hint = workspace_hint.clone();
            async move {
                match run_from_update(update, workspace_hint).await {
                    Ok(result) => Ok(Json(result)),
                    Err(error) => Err((axum::http::StatusCode::BAD_REQUEST, error)),
                }
            }
        }),
    );

    let listener = TcpListener::bind(bind).await?;
    tracing::info!(%bind, "serving Telegram webhook intake with full agent turns");
    axum::serve(listener, app).await?;
    Ok(())
}

pub async fn run_from_update(
    update: TelegramUpdate,
    workspace_hint: Option<String>,
) -> Result<AgentTurnResult, String> {
    let preview = prepare_from_update(update, workspace_hint).map_err(|error| error.to_string())?;
    let config = AppConfig::from_env(preview.prepared_turn_request.workspace_hint.clone())?;
    let thinkingroot = ThinkingRootSdkClient::new(config.thinkingroot.clone());
    let agent_identity =
        thinkingroot.derive_agent_identity(&preview.prepared_turn_request, None, None, None);
    let openai = OpenAiResponsesHttpClient::new(config.openai.clone())?;

    run_agent_turn(
        AgentTurnRequest {
            prepared_turn_request: preview.prepared_turn_request,
            agent_identity: Some(agent_identity),
            agent_id: None,
            prompt_name: None,
            session_id: None,
        },
        &thinkingroot,
        &openai,
        &config.agent,
        &config.openai,
    )
    .await
}

mod anyhow_free {
    pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_telegram_update_into_prepared_turn_request() {
        let raw = r#"{
            "update_id": 1,
            "message": {
                "message_id": 10,
                "from": {"id": 777, "username": "alen"},
                "chat": {"id": 12345, "type": "private"},
                "text": "  hello from telegram  "
            }
        }"#;
        let update: TelegramUpdate = serde_json::from_str(raw).unwrap();
        let preview = prepare_from_update(update, Some("ownpager".to_string())).unwrap();

        let request = preview.prepared_turn_request;
        assert_eq!(request.query_text, "hello from telegram");
        assert_eq!(request.session_key, "telegram:12345:777");
        assert_eq!(request.source.source_type, SourceType::Telegram);
        assert_eq!(request.source.chat_id.as_deref(), Some("12345"));
        assert_eq!(request.source.user_id.as_deref(), Some("777"));
    }

    #[test]
    fn rejects_update_without_text() {
        let raw = r#"{
            "update_id": 1,
            "message": {
                "message_id": 10,
                "from": {"id": 777},
                "chat": {"id": 12345, "type": "private"}
            }
        }"#;
        let update: TelegramUpdate = serde_json::from_str(raw).unwrap();
        assert_eq!(
            prepare_from_update(update, None).unwrap_err(),
            IntakeError::UnsupportedTelegramUpdate
        );
    }
}
