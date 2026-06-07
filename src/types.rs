use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceType {
    Cli,
    Telegram,
    Web,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalMode {
    PreTurn,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncomingUserQuery {
    pub request_id: Uuid,
    pub source_type: SourceType,
    pub raw_text: String,
    pub normalized_text: String,
    pub received_at: DateTime<Utc>,
    pub session_key: String,
    pub user_id: Option<String>,
    pub chat_id: Option<String>,
    pub message_id: Option<String>,
    pub cwd: Option<String>,
    pub workspace_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceMetadata {
    pub source_type: SourceType,
    pub user_id: Option<String>,
    pub chat_id: Option<String>,
    pub message_id: Option<String>,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreparedTurnRequest {
    pub request_id: Uuid,
    pub query_text: String,
    pub session_key: String,
    pub workspace_hint: Option<String>,
    pub source: SourceMetadata,
    pub retrieval_mode: RetrievalMode,
    pub branch_id: Option<String>,
    pub user_profile_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentIdentity {
    pub agent_id: String,
    pub scoped_user_id: String,
    pub workspace: String,
    pub prompt_name: Option<String>,
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThinkingRootClaim {
    pub claim_id: String,
    pub statement: String,
    pub claim_type: String,
    pub source_uri: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThinkingRootRoutedTool {
    pub name: String,
    pub score: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThinkingRootCapsule {
    pub system_prompt: String,
    pub grounded_claims: Vec<ThinkingRootClaim>,
    pub routed_tools: Vec<String>,
    pub token_estimate: usize,
    pub query_class: Option<String>,
    pub cache_hit: bool,
    pub frame_warm: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThinkingRootHotPathResult {
    pub agent_identity: AgentIdentity,
    pub capsule: ThinkingRootCapsule,
    pub route_tools: Vec<ThinkingRootRoutedTool>,
    pub cold_start_degraded: bool,
    pub used_route_fallback: bool,
    pub branch_run: Option<ThinkingRootBranchRun>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThinkingRootBranchRun {
    pub branch_id: String,
    pub parent: String,
    pub merge_policy: Option<String>,
    pub forked: bool,
    pub checked_out: bool,
    pub merged: bool,
    pub merge_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThinkingRootFlowRun {
    pub flow_id: String,
    pub flow_run_id: String,
    pub status: String,
    pub output: Option<Value>,
    pub raw: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreparedTurnPreview {
    pub prepared_turn_request: PreparedTurnRequest,
    pub thinkingroot_boundary: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTurnExecutionPlan {
    pub available_tool_names: Vec<String>,
    pub used_local_fallback: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTurnRequest {
    pub prepared_turn_request: PreparedTurnRequest,
    pub agent_identity: Option<AgentIdentity>,
    pub agent_id: Option<String>,
    pub prompt_name: Option<String>,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentToolExecutionRecord {
    pub tool_name: String,
    pub arguments: Value,
    pub result: Value,
    pub routed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostTurnKnowledgeCapture {
    pub request_id: Uuid,
    pub agent_identity: AgentIdentity,
    pub summary: Option<String>,
    pub facts: Vec<String>,
    pub decisions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThinkingRootCaptureResult {
    pub accepted: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTurnUsageEstimate {
    pub capsule_token_estimate: usize,
    pub api_call_count: usize,
    pub input_tokens: usize,
    pub output_tokens: usize,
    pub cached_input_tokens: usize,
    pub reasoning_output_tokens: usize,
    pub total_tokens: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentTurnResult {
    pub request_id: Uuid,
    pub final_answer: String,
    pub agent_identity: AgentIdentity,
    pub thinkingroot_hot_path: ThinkingRootHotPathResult,
    pub execution_plan: AgentTurnExecutionPlan,
    pub tool_calls: Vec<AgentToolExecutionRecord>,
    pub flow_run: Option<ThinkingRootFlowRun>,
    pub captured_knowledge: PostTurnKnowledgeCapture,
    pub capture_result: Option<ThinkingRootCaptureResult>,
    pub usage_estimate: AgentTurnUsageEstimate,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalCommandRequest {
    pub command: String,
    pub workdir: Option<String>,
    pub timeout_seconds: Option<u64>,
    pub max_output_bytes: Option<usize>,
    pub allowed_root: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalCommandStatus {
    Success,
    Failed,
    Blocked,
    TimedOut,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalSafetyDecision {
    pub allowed: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalCommandResult {
    pub command: String,
    pub cwd: String,
    pub status: TerminalCommandStatus,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u128,
    pub truncated: bool,
    pub safety: TerminalSafetyDecision,
}
