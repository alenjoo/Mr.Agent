use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;

use crate::config::ThinkingRootRuntimeConfig;
use crate::thinkingroot_boundary::ThinkingRootClient;
use crate::types::{
    PostTurnKnowledgeCapture, PreparedTurnRequest, PreparedTurnResponse, ThinkingRootCaptureResult,
    ThinkingRootKnowledgeStatus,
};

#[derive(Debug, Clone)]
pub struct HttpThinkingRootClient {
    client: Client,
    config: ThinkingRootRuntimeConfig,
}

impl HttpThinkingRootClient {
    pub fn new(config: ThinkingRootRuntimeConfig) -> Self {
        Self {
            client: Client::new(),
            config,
        }
    }

    fn base_url(&self) -> String {
        self.config.base_url.trim_end_matches('/').to_string()
    }
}

#[async_trait]
impl ThinkingRootClient for HttpThinkingRootClient {
    async fn prepare_turn(
        &self,
        request: PreparedTurnRequest,
    ) -> Result<PreparedTurnResponse, String> {
        let mut builder = self
            .client
            .post(format!("{}/prepare_turn", self.base_url()))
            .json(&request);
        if let Some(api_key) = self.config.api_key.as_ref() {
            builder = builder.bearer_auth(api_key);
        }

        let response = builder.send().await.map_err(|error| error.to_string())?;
        let status = response.status();
        let response: PreparedTurnResponse =
            response.json().await.map_err(|error| error.to_string())?;
        if !status.is_success() {
            return Err(format!("ThinkingRoot prepare_turn failed with {}", status));
        }
        Ok(classify_preparation(response))
    }

    async fn capture_turn(
        &self,
        capture: PostTurnKnowledgeCapture,
    ) -> Result<ThinkingRootCaptureResult, String> {
        let mut builder = self
            .client
            .post(format!("{}/capture_turn", self.base_url()))
            .json(&json!(capture));
        if let Some(api_key) = self.config.api_key.as_ref() {
            builder = builder.bearer_auth(api_key);
        }

        let response = builder.send().await.map_err(|error| error.to_string())?;
        let status = response.status();
        let response: ThinkingRootCaptureResult =
            response.json().await.map_err(|error| error.to_string())?;
        if !status.is_success() {
            return Err(format!("ThinkingRoot capture_turn failed with {}", status));
        }
        Ok(response)
    }
}

pub fn classify_preparation(mut response: PreparedTurnResponse) -> PreparedTurnResponse {
    response.knowledge_status = classify_knowledge_status(&response);
    response
}

pub fn classify_knowledge_status(response: &PreparedTurnResponse) -> ThinkingRootKnowledgeStatus {
    let memory = response.grounded_memory_block.trim();
    if memory.is_empty() && response.provenance_ids.is_empty() {
        return ThinkingRootKnowledgeStatus::Empty;
    }
    if memory.len() < 80 || response.provenance_ids.is_empty() {
        return ThinkingRootKnowledgeStatus::Weak;
    }
    ThinkingRootKnowledgeStatus::Sufficient
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PreparedTurnResponse;

    fn response(memory: &str, provenance_count: usize) -> PreparedTurnResponse {
        PreparedTurnResponse {
            grounded_memory_block: memory.to_string(),
            referenced_entities: vec![],
            provenance_ids: (0..provenance_count)
                .map(|index| format!("prov-{index}"))
                .collect(),
            warnings: vec![],
            knowledge_status: ThinkingRootKnowledgeStatus::Unavailable,
        }
    }

    #[test]
    fn classifies_empty_preparation() {
        assert_eq!(
            classify_knowledge_status(&response("", 0)),
            ThinkingRootKnowledgeStatus::Empty
        );
    }

    #[test]
    fn classifies_weak_preparation() {
        assert_eq!(
            classify_knowledge_status(&response("short memory", 1)),
            ThinkingRootKnowledgeStatus::Weak
        );
    }

    #[test]
    fn classifies_sufficient_preparation() {
        let memory =
            "This is a sufficiently detailed memory block that includes grounded context and recall.";
        assert_eq!(
            classify_knowledge_status(&response(memory, 2)),
            ThinkingRootKnowledgeStatus::Sufficient
        );
    }
}
