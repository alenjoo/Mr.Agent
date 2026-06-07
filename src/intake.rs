use chrono::Utc;
use thiserror::Error;
use uuid::Uuid;

use crate::types::{
    IncomingUserQuery, PreparedTurnRequest, RetrievalMode, SourceMetadata, SourceType,
};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum IntakeError {
    #[error("query text is empty")]
    EmptyMessage,
    #[error("unsupported Telegram update shape")]
    UnsupportedTelegramUpdate,
}

#[derive(Debug, Clone)]
pub struct IncomingQueryParts {
    pub source_type: SourceType,
    pub raw_text: String,
    pub session_key: String,
    pub user_id: Option<String>,
    pub chat_id: Option<String>,
    pub message_id: Option<String>,
    pub cwd: Option<String>,
    pub workspace_hint: Option<String>,
}

pub fn normalize_text(raw_text: &str) -> Result<String, IntakeError> {
    let normalized = raw_text
        .trim()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if normalized.is_empty() {
        return Err(IntakeError::EmptyMessage);
    }
    Ok(normalized)
}

pub fn build_incoming_query(parts: IncomingQueryParts) -> Result<IncomingUserQuery, IntakeError> {
    let normalized_text = normalize_text(&parts.raw_text)?;
    Ok(IncomingUserQuery {
        request_id: Uuid::new_v4(),
        source_type: parts.source_type,
        raw_text: parts.raw_text,
        normalized_text,
        received_at: Utc::now(),
        session_key: parts.session_key,
        user_id: parts.user_id,
        chat_id: parts.chat_id,
        message_id: parts.message_id,
        cwd: parts.cwd,
        workspace_hint: parts.workspace_hint,
    })
}

pub fn prepare_turn_request(query: IncomingUserQuery) -> PreparedTurnRequest {
    PreparedTurnRequest {
        request_id: query.request_id,
        query_text: query.normalized_text,
        session_key: query.session_key,
        workspace_hint: query.workspace_hint,
        source: SourceMetadata {
            source_type: query.source_type,
            user_id: query.user_id,
            chat_id: query.chat_id,
            message_id: query.message_id,
            cwd: query.cwd,
        },
        retrieval_mode: RetrievalMode::PreTurn,
        branch_id: None,
        user_profile_id: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_whitespace() {
        let normalized = normalize_text("  hello\n\n  world  ").unwrap();
        assert_eq!(normalized, "hello world");
    }

    #[test]
    fn rejects_empty_messages() {
        assert_eq!(normalize_text(" \n\t "), Err(IntakeError::EmptyMessage));
    }

    #[test]
    fn converts_incoming_query_to_prepared_turn_request() {
        let incoming = build_incoming_query(IncomingQueryParts {
            source_type: SourceType::Cli,
            raw_text: "  Explain this repo  ".to_string(),
            session_key: "cli:default:repo".to_string(),
            user_id: None,
            chat_id: None,
            message_id: None,
            cwd: Some("/repo".to_string()),
            workspace_hint: Some("repo".to_string()),
        })
        .unwrap();

        let prepared = prepare_turn_request(incoming);
        assert_eq!(prepared.query_text, "Explain this repo");
        assert_eq!(prepared.retrieval_mode, RetrievalMode::PreTurn);
        assert_eq!(prepared.source.source_type, SourceType::Cli);
    }
}
