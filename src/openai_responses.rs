use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::config::OpenAiRuntimeConfig;

#[derive(Debug, Clone)]
pub struct OpenAiResponsesHttpClient {
    client: Client,
    config: OpenAiRuntimeConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAiToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAiResponseRequest {
    pub instructions: String,
    pub input: Vec<Value>,
    pub tools: Vec<OpenAiToolDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAiFunctionCall {
    pub call_id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAiTokenUsage {
    pub input_tokens: usize,
    pub output_tokens: usize,
    pub cached_input_tokens: usize,
    pub reasoning_output_tokens: usize,
    pub total_tokens: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenAiResponse {
    pub output_text: String,
    pub output_items: Vec<Value>,
    pub function_calls: Vec<OpenAiFunctionCall>,
    pub usage: Option<OpenAiTokenUsage>,
}

#[async_trait]
pub trait OpenAiResponsesClient: Send + Sync {
    async fn create_response(
        &self,
        request: OpenAiResponseRequest,
    ) -> Result<OpenAiResponse, String>;
}

impl OpenAiResponsesHttpClient {
    pub fn new(config: OpenAiRuntimeConfig) -> Result<Self, String> {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_seconds))
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self { client, config })
    }
}

#[async_trait]
impl OpenAiResponsesClient for OpenAiResponsesHttpClient {
    async fn create_response(
        &self,
        request: OpenAiResponseRequest,
    ) -> Result<OpenAiResponse, String> {
        let payload = json!({
            "model": self.config.model,
            "instructions": request.instructions,
            "input": request.input,
            "tools": request.tools.iter().map(|tool| {
                json!({
                    "type": "function",
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.parameters,
                })
            }).collect::<Vec<_>>(),
        });

        let response = self
            .client
            .post(format!(
                "{}/responses",
                self.config.base_url.trim_end_matches('/')
            ))
            .bearer_auth(&self.config.api_key)
            .json(&payload)
            .send()
            .await
            .map_err(|error| error.to_string())?;

        let status = response.status();
        let value: Value = response.json().await.map_err(|error| error.to_string())?;
        if !status.is_success() {
            return Err(format!(
                "OpenAI Responses request failed with {}: {}",
                status, value
            ));
        }

        Ok(parse_openai_response(value))
    }
}

fn parse_openai_response(value: Value) -> OpenAiResponse {
    let output_items = value
        .get("output")
        .and_then(|output| output.as_array())
        .cloned()
        .unwrap_or_default();
    let output_text = value
        .get("output_text")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| extract_output_text(&output_items))
        .unwrap_or_default();
    let function_calls = output_items
        .iter()
        .filter_map(|item| {
            if item.get("type").and_then(Value::as_str) != Some("function_call") {
                return None;
            }
            Some(OpenAiFunctionCall {
                call_id: item
                    .get("call_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                name: item
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                arguments: item
                    .get("arguments")
                    .and_then(Value::as_str)
                    .unwrap_or("{}")
                    .to_string(),
            })
        })
        .collect();

    OpenAiResponse {
        output_text,
        output_items,
        function_calls,
        usage: parse_token_usage(value.get("usage")),
    }
}

fn parse_token_usage(value: Option<&Value>) -> Option<OpenAiTokenUsage> {
    let usage = value?;
    let input_tokens = usage
        .get("input_tokens")
        .or_else(|| usage.get("prompt_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    let output_tokens = usage
        .get("output_tokens")
        .or_else(|| usage.get("completion_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    let cached_input_tokens = usage
        .get("input_tokens_details")
        .or_else(|| usage.get("prompt_tokens_details"))
        .and_then(|details| {
            details
                .get("cached_tokens")
                .or_else(|| details.get("cache_read_tokens"))
        })
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    let reasoning_output_tokens = usage
        .get("output_tokens_details")
        .or_else(|| usage.get("completion_tokens_details"))
        .and_then(|details| details.get("reasoning_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    let total_tokens = usage
        .get("total_tokens")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(input_tokens + output_tokens);

    Some(OpenAiTokenUsage {
        input_tokens: input_tokens.saturating_sub(cached_input_tokens),
        output_tokens,
        cached_input_tokens,
        reasoning_output_tokens,
        total_tokens,
    })
}

fn extract_output_text(output_items: &[Value]) -> Option<String> {
    let mut parts = Vec::new();
    for item in output_items {
        if item.get("type").and_then(Value::as_str) != Some("message") {
            continue;
        }
        if let Some(content) = item.get("content").and_then(Value::as_array) {
            for block in content {
                if block.get("type").and_then(Value::as_str) == Some("output_text") {
                    if let Some(text) = block.get("text").and_then(Value::as_str) {
                        parts.push(text.to_string());
                    }
                }
            }
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_message_output_text() {
        let response = parse_openai_response(json!({
            "output": [
                {
                    "type": "message",
                    "content": [
                        {"type": "output_text", "text": "Hello world"}
                    ]
                }
            ]
        }));

        assert_eq!(response.output_text, "Hello world");
        assert!(response.function_calls.is_empty());
    }

    #[test]
    fn parses_function_calls() {
        let response = parse_openai_response(json!({
            "output": [
                {
                    "type": "function_call",
                    "call_id": "call_123",
                    "name": "terminal",
                    "arguments": "{\"command\":\"pwd\"}"
                }
            ]
        }));

        assert_eq!(response.function_calls.len(), 1);
        assert_eq!(response.function_calls[0].name, "terminal");
    }

    #[test]
    fn parses_response_usage() {
        let response = parse_openai_response(json!({
            "output_text": "Done",
            "usage": {
                "input_tokens": 100,
                "output_tokens": 25,
                "total_tokens": 125,
                "input_tokens_details": {
                    "cached_tokens": 40
                },
                "output_tokens_details": {
                    "reasoning_tokens": 10
                }
            }
        }));

        assert_eq!(
            response.usage,
            Some(OpenAiTokenUsage {
                input_tokens: 60,
                output_tokens: 25,
                cached_input_tokens: 40,
                reasoning_output_tokens: 10,
                total_tokens: 125,
            })
        );
    }
}
