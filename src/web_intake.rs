use std::net::SocketAddr;

use axum::extract::Json;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{options, post};
use axum::Router;
use serde::Deserialize;
use tokio::net::TcpListener;

use crate::agent_runner::run_agent_turn;
use crate::config::AppConfig;
use crate::intake::{build_incoming_query, prepare_turn_request, IncomingQueryParts, IntakeError};
use crate::openai_responses::OpenAiResponsesHttpClient;
use crate::session::derive_web_session_key;
use crate::thinkingroot_boundary::preview_before_thinkingroot;
use crate::thinkingroot_sdk::ThinkingRootSdkClient;
use crate::types::{AgentTurnRequest, AgentTurnResult, PreparedTurnPreview, SourceType};

#[derive(Debug, Deserialize)]
pub struct WebTurnRequest {
    pub message: String,
    #[serde(default = "default_profile")]
    pub profile: String,
    pub client_session_id: Option<String>,
}

pub fn prepare_from_web_request(
    request: WebTurnRequest,
    workspace_hint: Option<String>,
) -> Result<PreparedTurnPreview, IntakeError> {
    let profile = if request.profile.trim().is_empty() {
        default_profile()
    } else {
        request.profile.trim().to_string()
    };
    let session_key = derive_web_session_key(&profile, request.client_session_id.as_deref());
    let incoming = build_incoming_query(IncomingQueryParts {
        source_type: SourceType::Web,
        raw_text: request.message,
        session_key,
        user_id: Some(profile),
        chat_id: None,
        message_id: request.client_session_id,
        cwd: None,
        workspace_hint,
    })?;

    Ok(preview_before_thinkingroot(prepare_turn_request(incoming)))
}

pub async fn run_from_web_request(
    request: WebTurnRequest,
    workspace_hint: Option<String>,
) -> Result<AgentTurnResult, String> {
    let preview =
        prepare_from_web_request(request, workspace_hint).map_err(|error| error.to_string())?;
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

pub async fn serve_web(
    bind: SocketAddr,
    workspace_hint: Option<String>,
) -> anyhow_free::Result<()> {
    let app = Router::new()
        .route(
            "/api/turn",
            post(move |Json(request): Json<WebTurnRequest>| {
                let workspace_hint = workspace_hint.clone();
                async move {
                    match run_from_web_request(request, workspace_hint).await {
                        Ok(result) => cors(Json(result).into_response()),
                        Err(error) => cors((StatusCode::BAD_REQUEST, error).into_response()),
                    }
                }
            })
            .options(preflight),
        )
        .route(
            "/api/health",
            options(preflight).get(|| async { cors("ok".into_response()) }),
        );

    let listener = TcpListener::bind(bind).await?;
    tracing::info!(%bind, "serving web intake with full agent turns");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn preflight() -> Response {
    cors(StatusCode::NO_CONTENT.into_response())
}

fn cors(mut response: Response) -> Response {
    let headers = response.headers_mut();
    headers.insert("access-control-allow-origin", HeaderValue::from_static("*"));
    headers.insert(
        "access-control-allow-methods",
        HeaderValue::from_static("GET,POST,OPTIONS"),
    );
    headers.insert(
        "access-control-allow-headers",
        HeaderValue::from_static("content-type"),
    );
    headers.insert("access-control-max-age", HeaderValue::from_static("86400"));
    response
}

fn default_profile() -> String {
    "default".to_string()
}

mod anyhow_free {
    pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepares_web_request_as_web_source() {
        let preview = prepare_from_web_request(
            WebTurnRequest {
                message: "  hello from browser  ".to_string(),
                profile: "default".to_string(),
                client_session_id: Some("session-1".to_string()),
            },
            Some("ownpager".to_string()),
        )
        .unwrap();

        let request = preview.prepared_turn_request;
        assert_eq!(request.query_text, "hello from browser");
        assert_eq!(request.session_key, "web:default:session-1");
        assert_eq!(request.source.source_type, SourceType::Web);
        assert_eq!(request.source.user_id.as_deref(), Some("default"));
    }
}
