use std::path::Path;

use crate::types::SourceType;

pub fn derive_cli_session_key(profile: &str, cwd: Option<&str>) -> String {
    let normalized_profile = normalize_component(profile);
    let cwd_part = cwd
        .map(normalize_path_component)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown_cwd".to_string());
    format!("cli:{normalized_profile}:{cwd_part}")
}

pub fn derive_telegram_session_key(chat_id: &str, user_id: Option<&str>) -> String {
    let user_part = user_id
        .map(normalize_component)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown_user".to_string());
    format!("telegram:{}:{user_part}", normalize_component(chat_id))
}

pub fn derive_web_session_key(profile: &str, client_session_id: Option<&str>) -> String {
    let session_part = client_session_id
        .map(normalize_component)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "browser".to_string());
    format!("web:{}:{session_part}", normalize_component(profile))
}

pub fn derive_session_key(
    source_type: SourceType,
    profile: Option<&str>,
    chat_id: Option<&str>,
    user_id: Option<&str>,
    cwd: Option<&str>,
) -> String {
    match source_type {
        SourceType::Cli => derive_cli_session_key(profile.unwrap_or("default"), cwd),
        SourceType::Telegram => {
            derive_telegram_session_key(chat_id.unwrap_or("unknown_chat"), user_id)
        }
        SourceType::Web => derive_web_session_key(profile.unwrap_or("default"), user_id),
    }
}

fn normalize_path_component(path: &str) -> String {
    let expanded = Path::new(path);
    expanded
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
        .trim_matches('/')
        .replace([' ', ':'], "_")
}

fn normalize_component(value: &str) -> String {
    value.trim().replace([' ', ':'], "_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_cli_session_key_from_profile_and_cwd() {
        let key = derive_cli_session_key("default", Some("/tmp/my project"));
        assert_eq!(key, "cli:default:tmp/my_project");
    }

    #[test]
    fn derives_telegram_session_key_from_chat_and_user() {
        let key = derive_telegram_session_key("12345", Some("777"));
        assert_eq!(key, "telegram:12345:777");
    }

    #[test]
    fn derives_web_session_key_from_profile_and_browser_session() {
        let key = derive_web_session_key("default user", Some("browser:1"));
        assert_eq!(key, "web:default_user:browser_1");
    }
}
