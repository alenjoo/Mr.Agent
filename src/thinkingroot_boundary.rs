use async_trait::async_trait;

use serde_json::Value;

use crate::types::{
    AgentIdentity, PostTurnKnowledgeCapture, PreparedTurnPreview, PreparedTurnRequest,
    ThinkingRootCapsule, ThinkingRootCaptureResult, ThinkingRootFlowRun, ThinkingRootRoutedTool,
};

pub const PREPARE_TURN_BOUNDARY_MESSAGE: &str = "ThinkingRoot prepare_turn would be called here";

#[async_trait]
pub trait ThinkingRootClient: Send + Sync {
    async fn fork_branch(
        &self,
        branch_id: String,
        parent: String,
        description: Option<String>,
        merge_policy: Option<String>,
        identity: AgentIdentity,
    ) -> Result<(), String>;

    async fn checkout_branch(
        &self,
        branch_id: String,
        identity: AgentIdentity,
    ) -> Result<(), String>;

    async fn capsule(
        &self,
        request: PreparedTurnRequest,
        identity: AgentIdentity,
    ) -> Result<ThinkingRootCapsule, String>;

    async fn route(
        &self,
        query: String,
        identity: AgentIdentity,
        branch_id: Option<String>,
        top_k: usize,
    ) -> Result<Vec<ThinkingRootRoutedTool>, String>;

    async fn store_scoped(
        &self,
        capture: PostTurnKnowledgeCapture,
    ) -> Result<ThinkingRootCaptureResult, String>;

    async fn run_flow(
        &self,
        flow_id: String,
        inputs: Value,
        identity: AgentIdentity,
    ) -> Result<ThinkingRootFlowRun, String>;

    async fn flow_run(
        &self,
        flow_id: String,
        flow_run_id: String,
        identity: AgentIdentity,
    ) -> Result<ThinkingRootFlowRun, String>;

    async fn invoke_root_function(
        &self,
        function_name: String,
        input: Value,
        identity: AgentIdentity,
    ) -> Result<Value, String>;

    async fn merge_branch(
        &self,
        branch_id: String,
        merge_policy: Option<String>,
        identity: AgentIdentity,
    ) -> Result<(), String>;
}

pub fn preview_before_thinkingroot(request: PreparedTurnRequest) -> PreparedTurnPreview {
    PreparedTurnPreview {
        prepared_turn_request: request,
        thinkingroot_boundary: PREPARE_TURN_BOUNDARY_MESSAGE,
    }
}
