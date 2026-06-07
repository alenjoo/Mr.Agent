use std::env;

use crate::agent_runner::run_agent_turn;
use crate::config::AppConfig;
use crate::intake::{build_incoming_query, prepare_turn_request, IncomingQueryParts, IntakeError};
use crate::openai_responses::OpenAiResponsesHttpClient;
use crate::session::derive_cli_session_key;
use crate::thinkingroot_boundary::{preview_before_thinkingroot, PREPARE_TURN_BOUNDARY_MESSAGE};
use crate::thinkingroot_sdk::ThinkingRootSdkClient;
use crate::types::{AgentTurnRequest, AgentTurnResult, PreparedTurnPreview, SourceType};

pub fn handle_cli_message(
    message: String,
    profile: String,
    workspace_hint: Option<String>,
) -> Result<PreparedTurnPreview, IntakeError> {
    let cwd = env::current_dir()
        .ok()
        .map(|path| path.to_string_lossy().to_string());
    let session_key = derive_cli_session_key(&profile, cwd.as_deref());
    let incoming = build_incoming_query(IncomingQueryParts {
        source_type: SourceType::Cli,
        raw_text: message,
        session_key,
        user_id: Some(profile),
        chat_id: None,
        message_id: None,
        cwd,
        workspace_hint,
    })?;
    let prepared = prepare_turn_request(incoming);
    tracing::info!(
        boundary = PREPARE_TURN_BOUNDARY_MESSAGE,
        "prepared CLI query"
    );
    Ok(preview_before_thinkingroot(prepared))
}

pub async fn run_cli_message(
    message: String,
    profile: String,
    workspace_hint: Option<String>,
) -> Result<AgentTurnResult, String> {
    let preview =
        handle_cli_message(message, profile, workspace_hint).map_err(|error| error.to_string())?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepares_cli_message() {
        let preview = handle_cli_message(
            "  Build the bridge  ".to_string(),
            "default".to_string(),
            Some("ownpager".to_string()),
        )
        .unwrap();

        assert_eq!(preview.prepared_turn_request.query_text, "Build the bridge");
        assert_eq!(
            preview.prepared_turn_request.source.source_type,
            SourceType::Cli
        );
        assert_eq!(preview.thinkingroot_boundary, PREPARE_TURN_BOUNDARY_MESSAGE);
    }
}
