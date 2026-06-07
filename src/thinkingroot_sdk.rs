use std::path::Path;
use std::process::Stdio;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::config::ThinkingRootRuntimeConfig;
use crate::thinkingroot_boundary::ThinkingRootClient;
use crate::types::{
    AgentIdentity, PostTurnKnowledgeCapture, PreparedTurnRequest, ThinkingRootCapsule,
    ThinkingRootCaptureResult, ThinkingRootFlowRun, ThinkingRootRoutedTool,
};

#[derive(Debug, Clone)]
pub struct ThinkingRootSdkClient {
    config: ThinkingRootRuntimeConfig,
}

impl ThinkingRootSdkClient {
    pub fn new(config: ThinkingRootRuntimeConfig) -> Self {
        Self { config }
    }

    pub fn derive_agent_identity(
        &self,
        request: &PreparedTurnRequest,
        agent_id: Option<&str>,
        prompt_name: Option<&str>,
        session_id: Option<&str>,
    ) -> AgentIdentity {
        let agent_id = normalize_agent_id(
            agent_id.unwrap_or(self.config.default_agent_id.as_str()),
            self.config.default_agent_id.as_str(),
        );

        AgentIdentity {
            scoped_user_id: agent_id.clone(),
            agent_id,
            workspace: request
                .workspace_hint
                .clone()
                .unwrap_or_else(|| self.config.workspace.clone()),
            prompt_name: prompt_name
                .map(str::to_string)
                .or_else(|| Some(self.config.prompt_name.clone())),
            session_id: session_id
                .map(str::to_string)
                .unwrap_or_else(|| request.session_key.clone()),
        }
    }

    async fn invoke_bridge<TPayload, TResult>(
        &self,
        action: &str,
        payload: &TPayload,
    ) -> Result<TResult, String>
    where
        TPayload: Serialize,
        TResult: for<'de> Deserialize<'de>,
    {
        let script = Path::new(&self.config.sdk_bridge_script);
        if !script.exists() {
            return Err(format!(
                "ThinkingRoot SDK bridge script not found at {}",
                script.display()
            ));
        }

        let mut child = Command::new(&self.config.node_binary)
            .arg(script)
            .arg(action)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("failed to start ThinkingRoot SDK bridge: {error}"))?;

        let request_body = serde_json::to_vec(payload)
            .map_err(|error| format!("bridge payload error: {error}"))?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(&request_body)
                .await
                .map_err(|error| format!("bridge stdin error: {error}"))?;
        }

        let output = child
            .wait_with_output()
            .await
            .map_err(|error| format!("bridge execution error: {error}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let details = if !stderr.is_empty() { stderr } else { stdout };
            return Err(format!("ThinkingRoot SDK bridge failed: {details}"));
        }

        serde_json::from_slice::<TResult>(&output.stdout)
            .map_err(|error| format!("bridge response parse error: {error}"))
    }

    fn bridge_envelope(&self, identity: &AgentIdentity) -> BridgeEnvelope {
        BridgeEnvelope {
            gateway_url: self.config.gateway_url.clone(),
            project_key: self.config.project_key.clone(),
            workspace: identity.workspace.clone(),
            scoped_user_id: identity.scoped_user_id.clone(),
            prompt_name: identity.prompt_name.clone(),
            top_k: self.config.top_k,
            max_tools: self.config.max_tools,
            session_id: identity.session_id.clone(),
        }
    }
}

#[async_trait]
impl ThinkingRootClient for ThinkingRootSdkClient {
    async fn fork_branch(
        &self,
        branch_id: String,
        parent: String,
        description: Option<String>,
        merge_policy: Option<String>,
        identity: AgentIdentity,
    ) -> Result<(), String> {
        self.invoke_bridge::<_, serde_json::Value>(
            "fork_branch",
            &BranchBridgeRequest {
                envelope: self.bridge_envelope(&identity),
                branch_id,
                parent: Some(parent),
                description,
                merge_policy,
            },
        )
        .await
        .map(|_| ())
    }

    async fn checkout_branch(
        &self,
        branch_id: String,
        identity: AgentIdentity,
    ) -> Result<(), String> {
        self.invoke_bridge::<_, serde_json::Value>(
            "checkout_branch",
            &BranchBridgeRequest {
                envelope: self.bridge_envelope(&identity),
                branch_id,
                parent: None,
                description: None,
                merge_policy: None,
            },
        )
        .await
        .map(|_| ())
    }

    async fn capsule(
        &self,
        request: PreparedTurnRequest,
        identity: AgentIdentity,
    ) -> Result<ThinkingRootCapsule, String> {
        self.invoke_bridge::<_, ThinkingRootCapsule>(
            "capsule",
            &CapsuleBridgeRequest {
                envelope: self.bridge_envelope(&identity),
                query: request.query_text,
                branch_id: request.branch_id,
            },
        )
        .await
    }

    async fn route(
        &self,
        query: String,
        identity: AgentIdentity,
        branch_id: Option<String>,
        top_k: usize,
    ) -> Result<Vec<ThinkingRootRoutedTool>, String> {
        self.invoke_bridge::<_, Vec<ThinkingRootRoutedTool>>(
            "route",
            &RouteBridgeRequest {
                envelope: self.bridge_envelope(&identity),
                query,
                branch_id,
                top_k,
            },
        )
        .await
    }

    async fn store_scoped(
        &self,
        capture: PostTurnKnowledgeCapture,
    ) -> Result<ThinkingRootCaptureResult, String> {
        self.invoke_bridge::<_, ThinkingRootCaptureResult>(
            "store",
            &StoreBridgeRequest {
                envelope: self.bridge_envelope(&capture.agent_identity),
                capture,
            },
        )
        .await
    }

    async fn run_flow(
        &self,
        flow_id: String,
        inputs: serde_json::Value,
        identity: AgentIdentity,
    ) -> Result<ThinkingRootFlowRun, String> {
        self.invoke_bridge::<_, ThinkingRootFlowRun>(
            "run_flow",
            &FlowBridgeRequest {
                envelope: self.bridge_envelope(&identity),
                flow_id,
                flow_run_id: None,
                inputs: Some(inputs),
            },
        )
        .await
    }

    async fn flow_run(
        &self,
        flow_id: String,
        flow_run_id: String,
        identity: AgentIdentity,
    ) -> Result<ThinkingRootFlowRun, String> {
        self.invoke_bridge::<_, ThinkingRootFlowRun>(
            "flow_run",
            &FlowBridgeRequest {
                envelope: self.bridge_envelope(&identity),
                flow_id,
                flow_run_id: Some(flow_run_id),
                inputs: None,
            },
        )
        .await
    }

    async fn invoke_root_function(
        &self,
        function_name: String,
        input: serde_json::Value,
        identity: AgentIdentity,
    ) -> Result<serde_json::Value, String> {
        self.invoke_bridge::<_, serde_json::Value>(
            "invoke_function",
            &InvokeFunctionBridgeRequest {
                envelope: self.bridge_envelope(&identity),
                function_name,
                input,
            },
        )
        .await
    }

    async fn merge_branch(
        &self,
        branch_id: String,
        merge_policy: Option<String>,
        identity: AgentIdentity,
    ) -> Result<(), String> {
        self.invoke_bridge::<_, serde_json::Value>(
            "merge_branch",
            &BranchBridgeRequest {
                envelope: self.bridge_envelope(&identity),
                branch_id,
                parent: None,
                description: None,
                merge_policy,
            },
        )
        .await
        .map(|_| ())
    }
}

#[derive(Debug, Serialize)]
struct BridgeEnvelope {
    gateway_url: String,
    project_key: Option<String>,
    workspace: String,
    scoped_user_id: String,
    prompt_name: Option<String>,
    top_k: usize,
    max_tools: usize,
    session_id: String,
}

#[derive(Debug, Serialize)]
struct CapsuleBridgeRequest {
    envelope: BridgeEnvelope,
    query: String,
    branch_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct RouteBridgeRequest {
    envelope: BridgeEnvelope,
    query: String,
    branch_id: Option<String>,
    top_k: usize,
}

#[derive(Debug, Serialize)]
struct StoreBridgeRequest {
    envelope: BridgeEnvelope,
    capture: PostTurnKnowledgeCapture,
}

#[derive(Debug, Serialize)]
struct BranchBridgeRequest {
    envelope: BridgeEnvelope,
    branch_id: String,
    parent: Option<String>,
    description: Option<String>,
    merge_policy: Option<String>,
}

#[derive(Debug, Serialize)]
struct FlowBridgeRequest {
    envelope: BridgeEnvelope,
    flow_id: String,
    flow_run_id: Option<String>,
    inputs: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct InvokeFunctionBridgeRequest {
    envelope: BridgeEnvelope,
    function_name: String,
    input: serde_json::Value,
}

fn normalize_agent_id(raw: &str, fallback: &str) -> String {
    let normalized: String = raw
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' || character == '-' {
                character
            } else {
                '_'
            }
        })
        .collect();
    let normalized = normalized.trim_matches('_').to_string();
    if normalized.is_empty() {
        fallback.to_string()
    } else {
        normalized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{PreparedTurnRequest, RetrievalMode, SourceMetadata, SourceType};
    use uuid::Uuid;

    fn request() -> PreparedTurnRequest {
        PreparedTurnRequest {
            request_id: Uuid::nil(),
            query_text: "hello".to_string(),
            session_key: "telegram:chat:user".to_string(),
            workspace_hint: Some("ownpager".to_string()),
            source: SourceMetadata {
                source_type: SourceType::Telegram,
                user_id: Some("777".to_string()),
                chat_id: Some("123".to_string()),
                message_id: Some("10".to_string()),
                cwd: None,
            },
            retrieval_mode: RetrievalMode::PreTurn,
            branch_id: None,
            user_profile_id: None,
        }
    }

    fn client() -> ThinkingRootSdkClient {
        ThinkingRootSdkClient::new(ThinkingRootRuntimeConfig {
            gateway_url: "http://127.0.0.1:3001".to_string(),
            project_key: Some("tr_sk_test".to_string()),
            workspace: "ownpager".to_string(),
            default_agent_id: "ownpager".to_string(),
            prompt_name: "main".to_string(),
            top_k: 6,
            max_tools: 4,
            node_binary: "node".to_string(),
            sdk_bridge_script: "/tmp/bridge.mjs".to_string(),
        })
    }

    #[test]
    fn normalizes_agent_identity() {
        let identity = client().derive_agent_identity(&request(), Some("worker/main"), None, None);
        assert_eq!(identity.agent_id, "worker_main");
        assert_eq!(identity.scoped_user_id, "worker_main");
    }

    #[test]
    fn falls_back_to_default_agent_identity() {
        let identity = client().derive_agent_identity(&request(), None, None, None);
        assert_eq!(identity.agent_id, "ownpager");
        assert_eq!(identity.prompt_name.as_deref(), Some("main"));
        assert_eq!(identity.session_id, "telegram:chat:user");
    }
}
