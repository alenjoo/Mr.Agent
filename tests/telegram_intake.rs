use ownpager::telegram::{prepare_from_update, TelegramUpdate};
use ownpager::thinkingroot_boundary::PREPARE_TURN_BOUNDARY_MESSAGE;
use ownpager::types::{RetrievalMode, SourceType};

#[test]
fn telegram_update_is_ready_for_thinkingroot_without_invoking_it() {
    let raw = r#"{
        "update_id": 42,
        "message": {
            "message_id": 99,
            "from": {"id": 888, "username": "agent_user"},
            "chat": {"id": 555, "type": "private"},
            "text": "What does the bridge do?"
        }
    }"#;

    let update: TelegramUpdate = serde_json::from_str(raw).unwrap();
    let preview = prepare_from_update(update, Some("ownpager".to_string())).unwrap();
    let request = preview.prepared_turn_request;

    assert_eq!(preview.thinkingroot_boundary, PREPARE_TURN_BOUNDARY_MESSAGE);
    assert_eq!(request.query_text, "What does the bridge do?");
    assert_eq!(request.retrieval_mode, RetrievalMode::PreTurn);
    assert_eq!(request.source.source_type, SourceType::Telegram);
    assert_eq!(request.workspace_hint.as_deref(), Some("ownpager"));
}
