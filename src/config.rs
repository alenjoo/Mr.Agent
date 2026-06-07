use std::env;
use std::net::SocketAddr;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub default_workspace: Option<String>,
    pub thinkingroot: ThinkingRootRuntimeConfig,
    pub openai: OpenAiRuntimeConfig,
    pub agent: AgentRuntimeConfig,
}

#[derive(Debug, Clone)]
pub struct ThinkingRootRuntimeConfig {
    pub gateway_url: String,
    pub project_key: Option<String>,
    pub workspace: String,
    pub default_agent_id: String,
    pub prompt_name: String,
    pub top_k: usize,
    pub max_tools: usize,
    pub node_binary: String,
    pub sdk_bridge_script: String,
}

#[derive(Debug, Clone)]
pub struct OpenAiRuntimeConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone)]
pub struct AgentRuntimeConfig {
    pub max_tool_iterations: usize,
    pub routed_tool_top_k: usize,
    pub terminal_timeout_seconds: u64,
    pub terminal_max_output_bytes: usize,
    pub allow_local_terminal: bool,
    pub branch_per_run: bool,
    pub branch_parent: String,
    pub branch_merge_policy: Option<String>,
    pub flow_id: Option<String>,
    pub flow_mode: FlowMode,
    pub flow_poll_interval_ms: u64,
    pub flow_timeout_seconds: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowMode {
    Off,
    Auto,
    Always,
}

impl AppConfig {
    pub fn from_env(default_workspace: Option<String>) -> Result<Self, String> {
        let workspace_default = default_workspace.clone();
        Ok(Self {
            default_workspace,
            thinkingroot: ThinkingRootRuntimeConfig {
                gateway_url: env_or_default(
                    "THINKINGROOT_GATEWAY_URL",
                    env_or_default("THINKINGROOT_BASE_URL", "http://127.0.0.1:3001".to_string()),
                ),
                project_key: env::var("THINKINGROOT_PROJECT_KEY")
                    .ok()
                    .or_else(|| env::var("THINKINGROOT_API_KEY").ok())
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty()),
                workspace: env_or_default(
                    "THINKINGROOT_WORKSPACE",
                    workspace_default.unwrap_or_else(|| "ownpager".to_string()),
                ),
                default_agent_id: env_or_default(
                    "OWNPAGER_DEFAULT_AGENT_ID",
                    "ownpager".to_string(),
                ),
                prompt_name: env_or_default("THINKINGROOT_PROMPT_NAME", "main".to_string()),
                top_k: env::var("THINKINGROOT_TOP_K")
                    .ok()
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(6),
                max_tools: env::var("THINKINGROOT_MAX_TOOLS")
                    .ok()
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(4),
                node_binary: env_or_default("OWNPAGER_NODE_BINARY", "node".to_string()),
                sdk_bridge_script: env_or_default(
                    "OWNPAGER_THINKINGROOT_SDK_BRIDGE",
                    format!(
                        "{}/scripts/thinkingroot_sdk_bridge.mjs",
                        env!("CARGO_MANIFEST_DIR")
                    ),
                ),
            },
            openai: OpenAiRuntimeConfig::from_env()?,
            agent: AgentRuntimeConfig {
                max_tool_iterations: env::var("OWNPAGER_MAX_TOOL_ITERATIONS")
                    .ok()
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(3),
                routed_tool_top_k: env::var("OWNPAGER_ROUTED_TOOL_TOP_K")
                    .ok()
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(8),
                terminal_timeout_seconds: env::var("OWNPAGER_TERMINAL_TIMEOUT_SECONDS")
                    .ok()
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(30),
                terminal_max_output_bytes: env::var("OWNPAGER_TERMINAL_MAX_OUTPUT_BYTES")
                    .ok()
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(64 * 1024),
                allow_local_terminal: env::var("OWNPAGER_ALLOW_LOCAL_TERMINAL")
                    .ok()
                    .map(|value| parse_bool(&value))
                    .unwrap_or(false),
                branch_per_run: env::var("OWNPAGER_BRANCH_PER_RUN")
                    .ok()
                    .map(|value| parse_bool(&value))
                    .unwrap_or(true),
                branch_parent: env_or_default("OWNPAGER_BRANCH_PARENT", "main".to_string()),
                branch_merge_policy: env::var("OWNPAGER_BRANCH_MERGE_POLICY")
                    .ok()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty()),
                flow_id: env::var("OWNPAGER_FLOW_ID")
                    .ok()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty()),
                flow_mode: env::var("OWNPAGER_FLOW_MODE")
                    .ok()
                    .map(|value| parse_flow_mode(&value))
                    .unwrap_or(FlowMode::Auto),
                flow_poll_interval_ms: env::var("OWNPAGER_FLOW_POLL_INTERVAL_MS")
                    .ok()
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(1_000),
                flow_timeout_seconds: env::var("OWNPAGER_FLOW_TIMEOUT_SECONDS")
                    .ok()
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(180),
            },
        })
    }
}

impl OpenAiRuntimeConfig {
    pub fn from_env() -> Result<Self, String> {
        let api_key = env::var("OPENAI_API_KEY")
            .map_err(|_| "OPENAI_API_KEY is required for full agent turns".to_string())?;
        if api_key.trim().is_empty() {
            return Err("OPENAI_API_KEY is required for full agent turns".to_string());
        }

        Ok(Self {
            api_key,
            base_url: env_or_default("OPENAI_BASE_URL", "https://api.openai.com/v1".to_string()),
            model: env_or_default("OPENAI_MODEL", "gpt-4.1-mini".to_string()),
            timeout_seconds: env::var("OPENAI_TIMEOUT_SECONDS")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(60),
        })
    }
}

fn env_or_default(key: &str, default: String) -> String {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or(default)
}

fn parse_bool(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "y" | "on"
    )
}

fn parse_flow_mode(value: &str) -> FlowMode {
    match value.trim().to_ascii_lowercase().as_str() {
        "off" | "0" | "false" | "disabled" => FlowMode::Off,
        "always" | "force" | "1" | "true" => FlowMode::Always,
        _ => FlowMode::Auto,
    }
}

pub fn default_bind_addr() -> SocketAddr {
    "127.0.0.1:8080"
        .parse()
        .expect("default Telegram bind address must be valid")
}
