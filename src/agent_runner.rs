use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::time::{sleep, Duration, Instant};

use crate::config::{AgentRuntimeConfig, FlowMode, OpenAiRuntimeConfig};
use crate::openai_responses::{
    OpenAiResponseRequest, OpenAiResponsesClient, OpenAiTokenUsage, OpenAiToolDefinition,
};
use crate::terminal::run_terminal_command;
use crate::thinkingroot_boundary::ThinkingRootClient;
use crate::types::{
    AgentIdentity, AgentToolExecutionRecord, AgentTurnExecutionPlan, AgentTurnRequest,
    AgentTurnResult, AgentTurnUsageEstimate, PostTurnKnowledgeCapture, PreparedTurnRequest,
    TerminalCommandRequest, ThinkingRootBranchRun, ThinkingRootCapsule, ThinkingRootFlowRun,
    ThinkingRootHotPathResult,
};

const DEFAULT_BOOTSTRAP_TOOLS: &[&str] = &["terminal"];

pub async fn run_agent_turn(
    request: AgentTurnRequest,
    thinkingroot_client: &dyn ThinkingRootClient,
    openai_client: &dyn OpenAiResponsesClient,
    agent_config: &AgentRuntimeConfig,
    _openai_config: &OpenAiRuntimeConfig,
) -> Result<AgentTurnResult, String> {
    let mut prepared = request.prepared_turn_request;
    let identity = request
        .agent_identity
        .ok_or_else(|| "agent identity is required for the V3 hot path".to_string())?;

    let mut warnings = Vec::new();
    let mut branch_run = if agent_config.branch_per_run {
        let branch_id = prepared.branch_id.clone().unwrap_or_else(|| {
            derive_turn_branch_id(
                &identity.agent_id,
                &prepared.request_id.to_string(),
                &prepared.query_text,
            )
        });
        let description = Some(format!(
            "OwnPager isolated turn for request {}",
            prepared.request_id
        ));
        thinkingroot_client
            .fork_branch(
                branch_id.clone(),
                agent_config.branch_parent.clone(),
                description,
                agent_config.branch_merge_policy.clone(),
                identity.clone(),
            )
            .await
            .map_err(|error| format!("ThinkingRoot branch fork failed: {error}"))?;
        thinkingroot_client
            .checkout_branch(branch_id.clone(), identity.clone())
            .await
            .map_err(|error| format!("ThinkingRoot branch checkout failed: {error}"))?;
        prepared.branch_id = Some(branch_id.clone());
        Some(ThinkingRootBranchRun {
            branch_id,
            parent: agent_config.branch_parent.clone(),
            merge_policy: agent_config.branch_merge_policy.clone(),
            forked: true,
            checked_out: true,
            merged: false,
            merge_error: None,
        })
    } else {
        None
    };

    let capsule = thinkingroot_client
        .capsule(prepared.clone(), identity.clone())
        .await
        .map_err(|error| format!("ThinkingRoot capsule failed: {error}"))?;
    let mut usage_estimate = AgentTurnUsageEstimate {
        capsule_token_estimate: capsule.token_estimate,
        api_call_count: 0,
        input_tokens: 0,
        output_tokens: 0,
        cached_input_tokens: 0,
        reasoning_output_tokens: 0,
        total_tokens: 0,
    };

    warnings.extend(capsule.warnings.clone());
    if capsule.system_prompt.trim().is_empty() {
        warnings.push(format!(
            "ThinkingRoot prompt `{}` compiled to an empty system frame",
            identity.prompt_name.as_deref().unwrap_or("<unset>")
        ));
    }

    if let Some(flow_id) = select_flow_id(agent_config, &prepared.query_text) {
        let flow_run = run_thinkingroot_flow(
            thinkingroot_client,
            flow_id,
            &prepared,
            &identity,
            &capsule,
            agent_config,
        )
        .await?;
        let final_answer = flow_final_answer(&flow_run);
        let tool_calls = Vec::new();
        let captured_knowledge =
            build_post_turn_capture(&prepared, &identity, &final_answer, &tool_calls);
        let capture_result = thinkingroot_client
            .store_scoped(captured_knowledge.clone())
            .await
            .map(Some)
            .unwrap_or_else(|error| {
                warnings.push(format!("ThinkingRoot scoped store failed: {error}"));
                None
            });

        if let Some(branch) = branch_run.as_mut() {
            match thinkingroot_client
                .merge_branch(
                    branch.branch_id.clone(),
                    branch.merge_policy.clone(),
                    identity.clone(),
                )
                .await
            {
                Ok(()) => {
                    branch.merged = true;
                }
                Err(error) => {
                    branch.merge_error = Some(error.clone());
                    warnings.push(format!("ThinkingRoot branch merge failed: {error}"));
                }
            }
        }

        return Ok(AgentTurnResult {
            request_id: prepared.request_id,
            final_answer,
            agent_identity: identity.clone(),
            thinkingroot_hot_path: ThinkingRootHotPathResult {
                agent_identity: identity,
                capsule,
                route_tools: vec![],
                cold_start_degraded: false,
                used_route_fallback: false,
                branch_run,
                warnings: warnings.clone(),
            },
            execution_plan: AgentTurnExecutionPlan {
                available_tool_names: vec![],
                used_local_fallback: false,
            },
            tool_calls,
            flow_run: Some(flow_run),
            captured_knowledge,
            capture_result,
            usage_estimate,
            warnings,
        });
    }

    let mut route_tools = Vec::new();
    let mut cold_start_degraded = false;
    let mut used_route_fallback = false;
    let mut used_local_fallback = false;
    let has_grounding =
        !capsule.grounded_claims.is_empty() || !capsule.system_prompt.trim().is_empty();

    let should_bootstrap_terminal = capsule.routed_tools.is_empty()
        && agent_config.allow_local_terminal
        && (!has_grounding || asks_for_terminal_or_workspace_inspection(&prepared.query_text));
    let mut allowed_tool_names = if should_bootstrap_terminal {
        cold_start_degraded = !has_grounding;
        used_local_fallback = true;
        DEFAULT_BOOTSTRAP_TOOLS
            .iter()
            .map(|tool| (*tool).to_string())
            .collect::<Vec<_>>()
    } else {
        capsule.routed_tools.clone()
    };

    if capsule.routed_tools.is_empty() {
        match thinkingroot_client
            .route(
                prepared.query_text.clone(),
                identity.clone(),
                prepared.branch_id.clone(),
                agent_config.routed_tool_top_k,
            )
            .await
        {
            Ok(ranked) => {
                if !ranked.is_empty() {
                    used_route_fallback = true;
                    route_tools = ranked;
                    allowed_tool_names = route_tools.iter().map(|tool| tool.name.clone()).collect();
                    if allowed_tool_names.is_empty() && should_bootstrap_terminal {
                        cold_start_degraded = !has_grounding;
                        used_local_fallback = true;
                        allowed_tool_names = DEFAULT_BOOTSTRAP_TOOLS
                            .iter()
                            .map(|tool| (*tool).to_string())
                            .collect();
                    }
                }
            }
            Err(error) => warnings.push(format!("ThinkingRoot route failed: {error}")),
        }
    }

    let hot_path = ThinkingRootHotPathResult {
        agent_identity: identity.clone(),
        capsule: capsule.clone(),
        route_tools: route_tools.clone(),
        cold_start_degraded,
        used_route_fallback,
        branch_run: branch_run.clone(),
        warnings: warnings.clone(),
    };

    let registry = ToolRegistry::default();
    let execution_plan =
        registry.build_execution_plan(&allowed_tool_names, used_local_fallback, agent_config);
    warnings.extend(execution_plan.warnings.clone());

    let instructions = build_instructions(&capsule, !execution_plan.tool_definitions.is_empty());
    let mut input = vec![user_message(&prepared.query_text)];
    let mut tool_calls = Vec::new();

    for step in 0..=agent_config.max_tool_iterations {
        let response = openai_client
            .create_response(OpenAiResponseRequest {
                instructions: instructions.clone(),
                input: input.clone(),
                tools: execution_plan.tool_definitions.clone(),
            })
            .await?;
        usage_estimate.api_call_count += 1;
        merge_openai_usage(&mut usage_estimate, response.usage.as_ref());

        input.extend(response.output_items.clone());

        if response.function_calls.is_empty() {
            let final_answer = if response.output_text.trim().is_empty() {
                "No answer was produced.".to_string()
            } else {
                response.output_text
            };

            let captured_knowledge =
                build_post_turn_capture(&prepared, &identity, &final_answer, &tool_calls);
            let capture_result = thinkingroot_client
                .store_scoped(captured_knowledge.clone())
                .await
                .map(Some)
                .unwrap_or_else(|error| {
                    warnings.push(format!("ThinkingRoot scoped store failed: {error}"));
                    None
                });

            if let Some(branch) = branch_run.as_mut() {
                match thinkingroot_client
                    .merge_branch(
                        branch.branch_id.clone(),
                        branch.merge_policy.clone(),
                        identity.clone(),
                    )
                    .await
                {
                    Ok(()) => {
                        branch.merged = true;
                    }
                    Err(error) => {
                        branch.merge_error = Some(error.clone());
                        warnings.push(format!("ThinkingRoot branch merge failed: {error}"));
                    }
                }
            }

            return Ok(AgentTurnResult {
                request_id: prepared.request_id,
                final_answer,
                agent_identity: identity,
                thinkingroot_hot_path: ThinkingRootHotPathResult {
                    branch_run,
                    ..hot_path
                },
                execution_plan: execution_plan.summary(),
                tool_calls,
                flow_run: None,
                captured_knowledge,
                capture_result,
                usage_estimate,
                warnings,
            });
        }

        if step == agent_config.max_tool_iterations {
            warnings.push(format!(
                "Agent stopped after reaching the max tool iteration limit ({}).",
                agent_config.max_tool_iterations
            ));
            let final_answer =
                build_tool_iteration_limit_answer(agent_config.max_tool_iterations, &tool_calls);
            let captured_knowledge =
                build_post_turn_capture(&prepared, &identity, &final_answer, &tool_calls);
            let capture_result = thinkingroot_client
                .store_scoped(captured_knowledge.clone())
                .await
                .map(Some)
                .unwrap_or_else(|error| {
                    warnings.push(format!("ThinkingRoot scoped store failed: {error}"));
                    None
                });

            if let Some(branch) = branch_run.as_mut() {
                match thinkingroot_client
                    .merge_branch(
                        branch.branch_id.clone(),
                        branch.merge_policy.clone(),
                        identity.clone(),
                    )
                    .await
                {
                    Ok(()) => {
                        branch.merged = true;
                    }
                    Err(error) => {
                        branch.merge_error = Some(error.clone());
                        warnings.push(format!("ThinkingRoot branch merge failed: {error}"));
                    }
                }
            }

            return Ok(AgentTurnResult {
                request_id: prepared.request_id,
                final_answer,
                agent_identity: identity,
                thinkingroot_hot_path: ThinkingRootHotPathResult {
                    branch_run,
                    ..hot_path
                },
                execution_plan: execution_plan.summary(),
                tool_calls,
                flow_run: None,
                captured_knowledge,
                capture_result,
                usage_estimate,
                warnings,
            });
        }

        for function_call in response.function_calls {
            let execution = registry
                .execute(
                    &execution_plan,
                    &function_call.name,
                    &function_call.arguments,
                    &prepared,
                    thinkingroot_client,
                    &identity,
                    agent_config.terminal_timeout_seconds,
                    agent_config.terminal_max_output_bytes,
                )
                .await?;

            let result_value = execution.result.clone();
            tool_calls.push(AgentToolExecutionRecord {
                tool_name: execution.tool_name.clone(),
                arguments: execution.arguments.clone(),
                result: result_value.clone(),
                routed: execution.routed,
            });
            input.push(function_call_output(&function_call.call_id, result_value));
        }
    }

    Err("agent finished without producing an answer".to_string())
}

fn merge_openai_usage(target: &mut AgentTurnUsageEstimate, usage: Option<&OpenAiTokenUsage>) {
    if let Some(usage) = usage {
        target.input_tokens += usage.input_tokens;
        target.output_tokens += usage.output_tokens;
        target.cached_input_tokens += usage.cached_input_tokens;
        target.reasoning_output_tokens += usage.reasoning_output_tokens;
        target.total_tokens += usage.total_tokens;
    }
}

fn build_instructions(capsule: &ThinkingRootCapsule, has_tools: bool) -> String {
    let mut instructions = capsule.system_prompt.trim().to_string();
    if has_tools {
        instructions.push_str(
            "\n\nTool discipline: use the terminal only for necessary evidence. \
             Prefer one combined read-only command over many small commands. \
             After gathering enough evidence, stop calling tools and produce the final answer.",
        );
    }
    instructions
}

fn select_flow_id<'a>(agent_config: &'a AgentRuntimeConfig, query_text: &str) -> Option<&'a str> {
    let flow_id = agent_config.flow_id.as_deref()?;
    match agent_config.flow_mode {
        FlowMode::Off => None,
        FlowMode::Always => Some(flow_id),
        FlowMode::Auto if looks_like_complex_task(query_text) => Some(flow_id),
        FlowMode::Auto => None,
    }
}

fn looks_like_complex_task(query_text: &str) -> bool {
    let normalized = query_text.to_ascii_lowercase();
    let explicit_sequence = [" then ", " after that ", " finally ", "step by step"]
        .iter()
        .any(|marker| normalized.contains(marker));
    if explicit_sequence {
        return true;
    }

    let task_markers = [
        "research",
        "competitor",
        "report",
        "email",
        "write",
        "summarize",
        "compare",
        "plan",
        "design",
        "implement",
        "analyze",
    ];
    let marker_count = task_markers
        .iter()
        .filter(|marker| normalized.contains(**marker))
        .count();
    marker_count >= 2
}

fn asks_for_terminal_or_workspace_inspection(query_text: &str) -> bool {
    let normalized = query_text.to_ascii_lowercase();
    let has_execution_marker = [
        "terminal",
        "tooling",
        "command",
        "inspect",
        "list",
        "current directory",
        "workspace",
        "files",
        "folders",
        "package",
        "config",
    ]
    .iter()
    .any(|marker| normalized.contains(marker));
    let has_local_target = [
        "/users/",
        "repo",
        "repository",
        "project",
        "directory",
        "folder",
        "workspace",
    ]
    .iter()
    .any(|marker| normalized.contains(marker));

    has_execution_marker && has_local_target
}

async fn run_thinkingroot_flow(
    thinkingroot_client: &dyn ThinkingRootClient,
    flow_id: &str,
    prepared: &PreparedTurnRequest,
    identity: &AgentIdentity,
    capsule: &ThinkingRootCapsule,
    agent_config: &AgentRuntimeConfig,
) -> Result<ThinkingRootFlowRun, String> {
    let inputs = json!({
        "request_id": prepared.request_id,
        "query": prepared.query_text,
        "session_key": prepared.session_key,
        "workspace_hint": prepared.workspace_hint,
        "branch_id": prepared.branch_id,
        "source": prepared.source,
        "agent_identity": identity,
        "prompt_name": identity.prompt_name,
        "capsule": {
            "system_prompt": capsule.system_prompt,
            "grounded_claims": capsule.grounded_claims,
            "routed_tools": capsule.routed_tools,
            "query_class": capsule.query_class,
        }
    });
    let mut snapshot = thinkingroot_client
        .run_flow(flow_id.to_string(), inputs, identity.clone())
        .await
        .map_err(|error| format!("ThinkingRoot flow start failed: {error}"))?;
    let deadline = Instant::now() + Duration::from_secs(agent_config.flow_timeout_seconds);

    loop {
        let status = snapshot.status.to_ascii_lowercase();
        if is_flow_success_status(&status) {
            return Ok(snapshot);
        }
        if is_flow_failure_status(&status) {
            return Err(format!(
                "ThinkingRoot flow `{}` failed with status `{}`",
                flow_id, snapshot.status
            ));
        }
        if snapshot.flow_run_id.trim().is_empty() {
            return Err(format!(
                "ThinkingRoot flow `{flow_id}` did not return a flow_run_id"
            ));
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "ThinkingRoot flow `{}` timed out after {} seconds",
                flow_id, agent_config.flow_timeout_seconds
            ));
        }

        sleep(Duration::from_millis(agent_config.flow_poll_interval_ms)).await;
        snapshot = thinkingroot_client
            .flow_run(
                flow_id.to_string(),
                snapshot.flow_run_id.clone(),
                identity.clone(),
            )
            .await
            .map_err(|error| format!("ThinkingRoot flow poll failed: {error}"))?;
    }
}

fn is_flow_success_status(status: &str) -> bool {
    matches!(
        status,
        "completed" | "complete" | "succeeded" | "success" | "finished" | "done"
    )
}

fn is_flow_failure_status(status: &str) -> bool {
    matches!(
        status,
        "failed" | "error" | "errored" | "cancelled" | "canceled" | "timed_out" | "timeout"
    )
}

fn flow_final_answer(flow_run: &ThinkingRootFlowRun) -> String {
    if let Some(output) = flow_run.output.as_ref() {
        if let Some(answer) = output.as_str() {
            if !answer.trim().is_empty() {
                return answer.trim().to_string();
            }
        }
        for key in ["final_answer", "answer", "summary", "text", "report"] {
            if let Some(answer) = output.get(key).and_then(Value::as_str) {
                if !answer.trim().is_empty() {
                    return answer.trim().to_string();
                }
            }
        }
        if !output.is_null() {
            return output.to_string();
        }
    }

    format!(
        "ThinkingRoot flow `{}` completed with status `{}`.",
        flow_run.flow_id, flow_run.status
    )
}

fn build_tool_iteration_limit_answer(
    max_tool_iterations: usize,
    tool_calls: &[AgentToolExecutionRecord],
) -> String {
    let mut lines = vec![format!(
        "Stopped after reaching the max tool iteration limit ({max_tool_iterations}). Here is the evidence gathered so far."
    )];

    for (index, call) in tool_calls.iter().rev().take(5).enumerate() {
        lines.push(format!(
            "\nTool call {}: {}",
            tool_calls.len().saturating_sub(index),
            call.tool_name
        ));
        if let Some(command) = call.arguments.get("command").and_then(Value::as_str) {
            lines.push(format!("Command: {command}"));
        }
        if let Some(cwd) = call.result.get("cwd").and_then(Value::as_str) {
            lines.push(format!("Working directory: {cwd}"));
        }
        if let Some(status) = call.result.get("status").and_then(Value::as_str) {
            lines.push(format!("Status: {status}"));
        }
        if let Some(stdout) = call.result.get("stdout").and_then(Value::as_str) {
            let trimmed = stdout.trim();
            if !trimmed.is_empty() {
                lines.push(format!("Output:\n{}", truncate_for_answer(trimmed, 1_200)));
            }
        }
        if let Some(stderr) = call.result.get("stderr").and_then(Value::as_str) {
            let trimmed = stderr.trim();
            if !trimmed.is_empty() {
                lines.push(format!(
                    "Error output:\n{}",
                    truncate_for_answer(trimmed, 800)
                ));
            }
        }
    }

    if tool_calls.is_empty() {
        lines.push("No tool output was captured before the limit was reached.".to_string());
    }

    lines.join("\n")
}

fn truncate_for_answer(value: &str, max_chars: usize) -> String {
    let mut truncated = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        truncated.push_str("\n...");
    }
    truncated
}
fn build_terminal_request(
    arguments: &Value,
    prepared: &PreparedTurnRequest,
    default_timeout_seconds: u64,
    default_max_output_bytes: usize,
) -> Result<TerminalCommandRequest, String> {
    let command = arguments
        .get("command")
        .and_then(Value::as_str)
        .ok_or_else(|| "terminal tool call missing command".to_string())?
        .to_string();
    let workdir = arguments
        .get("workdir")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| prepared.source.cwd.clone());
    let timeout_seconds = arguments
        .get("timeout_seconds")
        .and_then(Value::as_u64)
        .or(Some(default_timeout_seconds));
    let max_output_bytes = arguments
        .get("max_output_bytes")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .or(Some(default_max_output_bytes));

    Ok(TerminalCommandRequest {
        command,
        workdir,
        timeout_seconds,
        max_output_bytes,
        allowed_root: None,
    })
}

fn build_post_turn_capture(
    prepared: &PreparedTurnRequest,
    identity: &AgentIdentity,
    final_answer: &str,
    tool_calls: &[AgentToolExecutionRecord],
) -> PostTurnKnowledgeCapture {
    let facts = extract_sentences(final_answer);
    let mut decisions = Vec::new();
    if !tool_calls.is_empty() {
        decisions.push("Used routed tools to gather live evidence.".to_string());
    }
    if prepared.branch_id.is_some() {
        decisions.push("Handled a branch-scoped turn request.".to_string());
    }

    PostTurnKnowledgeCapture {
        request_id: prepared.request_id,
        agent_identity: identity.clone(),
        summary: Some(final_answer.trim().to_string()),
        facts,
        decisions,
    }
}

fn derive_turn_branch_id(agent_id: &str, request_id: &str, query_text: &str) -> String {
    let query_slug = query_text
        .chars()
        .take(32)
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let query_slug = if query_slug.is_empty() {
        "turn".to_string()
    } else {
        query_slug
    };
    let short_request_id = request_id
        .chars()
        .filter(|character| character.is_ascii_hexdigit())
        .take(8)
        .collect::<String>();
    format!(
        "ownpager/{}/{}-{}",
        sanitize_branch_component(agent_id),
        query_slug,
        short_request_id
    )
}

fn sanitize_branch_component(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' || character == '-' {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if sanitized.is_empty() {
        "agent".to_string()
    } else {
        sanitized
    }
}

fn extract_sentences(text: &str) -> Vec<String> {
    text.split(['.', '\n'])
        .map(str::trim)
        .filter(|sentence| sentence.len() >= 20)
        .filter(|sentence| !sentence.ends_with('?'))
        .map(str::to_string)
        .take(5)
        .collect()
}

fn user_message(prompt: &str) -> Value {
    json!({
        "role": "user",
        "content": [
            {
                "type": "input_text",
                "text": prompt
            }
        ]
    })
}

fn function_call_output(call_id: &str, output: Value) -> Value {
    json!({
        "type": "function_call_output",
        "call_id": call_id,
        "output": output.to_string()
    })
}

#[derive(Debug, Clone)]
struct ExecutionPlan {
    tool_definitions: Vec<OpenAiToolDefinition>,
    available_tool_names: Vec<String>,
    allowed_routes: HashSet<String>,
    root_function_routes: HashMap<String, String>,
    used_local_fallback: bool,
    warnings: Vec<String>,
}

impl ExecutionPlan {
    fn summary(&self) -> AgentTurnExecutionPlan {
        AgentTurnExecutionPlan {
            available_tool_names: self.available_tool_names.clone(),
            used_local_fallback: self.used_local_fallback,
        }
    }
}

#[derive(Debug)]
struct ToolExecutionOutcome {
    tool_name: String,
    arguments: Value,
    result: Value,
    routed: bool,
}

#[derive(Debug, Clone)]
enum ToolSpecKind {
    LocalTerminal,
}

#[derive(Debug, Clone)]
struct ToolSpec {
    canonical_name: String,
    route_aliases: Vec<String>,
    definition: OpenAiToolDefinition,
    kind: ToolSpecKind,
}

#[derive(Debug, Default)]
struct ToolRegistry {
    specs: Vec<ToolSpec>,
}

impl ToolRegistry {
    fn default() -> Self {
        Self {
            specs: vec![ToolSpec {
                canonical_name: "terminal".to_string(),
                route_aliases: vec!["terminal".to_string(), "function::terminal".to_string()],
                definition: OpenAiToolDefinition {
                    name: "terminal".to_string(),
                    description:
                        "Run a local shell command to gather live evidence from the workspace."
                            .to_string(),
                    parameters: json!({
                        "type": "object",
                        "additionalProperties": false,
                        "properties": {
                            "command": {"type": "string", "description": "Shell command to run."},
                            "workdir": {"type": "string", "description": "Working directory inside the allowed workspace root."},
                            "timeout_seconds": {"type": "integer", "description": "Optional timeout in seconds."},
                            "max_output_bytes": {"type": "integer", "description": "Optional max combined output size."}
                        },
                        "required": ["command"]
                    }),
                },
                kind: ToolSpecKind::LocalTerminal,
            }],
        }
    }

    fn build_execution_plan(
        &self,
        routed_tool_names: &[String],
        used_local_fallback: bool,
        agent_config: &AgentRuntimeConfig,
    ) -> ExecutionPlan {
        let mut tool_definitions = Vec::new();
        let mut available_tool_names = Vec::new();
        let mut allowed_routes = HashSet::new();
        let mut root_function_routes = HashMap::new();
        let mut warnings = Vec::new();

        for routed_name in routed_tool_names {
            if let Some(spec) = self.find_spec(routed_name) {
                if !agent_config.allow_local_terminal {
                    warnings.push(format!(
                        "ThinkingRoot routed local tool `{}` but local terminal execution is disabled.",
                        routed_name
                    ));
                    continue;
                }
                if allowed_routes.insert(spec.canonical_name.clone()) {
                    tool_definitions.push(spec.definition.clone());
                    available_tool_names.push(spec.canonical_name.clone());
                }
                allowed_routes.insert(routed_name.clone());
                continue;
            }

            let tool_name = openai_root_function_tool_name(routed_name);
            if root_function_routes.contains_key(&tool_name) {
                warnings.push(format!(
                    "ThinkingRoot routed `{}` but its OpenAI tool name collided with another Root Function.",
                    routed_name
                ));
                continue;
            }
            if allowed_routes.insert(tool_name.clone()) {
                root_function_routes.insert(tool_name.clone(), routed_name.clone());
                available_tool_names.push(routed_name.clone());
                tool_definitions.push(root_function_tool_definition(&tool_name, routed_name));
            }
        }

        ExecutionPlan {
            tool_definitions,
            available_tool_names,
            allowed_routes,
            root_function_routes,
            used_local_fallback,
            warnings,
        }
    }

    async fn execute(
        &self,
        plan: &ExecutionPlan,
        function_name: &str,
        arguments: &str,
        prepared: &PreparedTurnRequest,
        thinkingroot_client: &dyn ThinkingRootClient,
        identity: &AgentIdentity,
        default_timeout_seconds: u64,
        default_max_output_bytes: usize,
    ) -> Result<ToolExecutionOutcome, String> {
        let parsed_arguments: Value =
            serde_json::from_str(arguments).map_err(|error| error.to_string())?;

        if !plan.allowed_routes.contains(function_name) {
            return Ok(ToolExecutionOutcome {
                tool_name: function_name.to_string(),
                arguments: parsed_arguments,
                result: json!({ "error": format!("tool `{}` was not routed for this turn", function_name) }),
                routed: false,
            });
        }

        if let Some(root_function_name) = plan.root_function_routes.get(function_name) {
            let input = parsed_arguments
                .get("input")
                .cloned()
                .unwrap_or_else(|| parsed_arguments.clone());
            let result = thinkingroot_client
                .invoke_root_function(root_function_name.clone(), input, identity.clone())
                .await
                .map_err(|error| {
                    format!("ThinkingRoot Root Function `{root_function_name}` failed: {error}")
                })?;
            return Ok(ToolExecutionOutcome {
                tool_name: root_function_name.clone(),
                arguments: parsed_arguments,
                result,
                routed: true,
            });
        }

        let Some(spec) = self.find_spec(function_name) else {
            return Ok(ToolExecutionOutcome {
                tool_name: function_name.to_string(),
                arguments: parsed_arguments,
                result: json!({ "error": format!("tool `{}` was routed but no Root Function mapping exists", function_name) }),
                routed: true,
            });
        };

        match spec.kind {
            ToolSpecKind::LocalTerminal => {
                let request = build_terminal_request(
                    &parsed_arguments,
                    prepared,
                    default_timeout_seconds,
                    default_max_output_bytes,
                )?;
                let result = run_terminal_command(request);
                Ok(ToolExecutionOutcome {
                    tool_name: spec.canonical_name.clone(),
                    arguments: parsed_arguments,
                    result: serde_json::to_value(result).map_err(|error| error.to_string())?,
                    routed: true,
                })
            }
        }
    }

    fn find_spec(&self, routed_name: &str) -> Option<&ToolSpec> {
        self.specs.iter().find(|spec| {
            spec.canonical_name == routed_name
                || spec.route_aliases.iter().any(|alias| alias == routed_name)
        })
    }
}

fn openai_root_function_tool_name(function_name: &str) -> String {
    let mut name = String::from("tr_");
    for character in function_name.chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            name.push(character.to_ascii_lowercase());
        } else {
            name.push('_');
        }
    }
    let collapsed = name
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    let safe_name = if collapsed.is_empty() {
        "tr_root_function".to_string()
    } else if collapsed.starts_with("tr_") {
        collapsed
    } else {
        format!("tr_{collapsed}")
    };
    safe_name.chars().take(64).collect()
}

fn root_function_tool_definition(tool_name: &str, function_name: &str) -> OpenAiToolDefinition {
    OpenAiToolDefinition {
        name: tool_name.to_string(),
        description: format!(
            "Invoke the ThinkingRoot Root Function `{function_name}` in the cloud sandbox. Pass the function input as JSON in `input`."
        ),
        parameters: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "input": {
                    "type": "object",
                    "description": "JSON payload for the Root Function.",
                    "additionalProperties": true
                }
            },
            "required": ["input"]
        }),
    }
}

#[async_trait]
pub trait ThinkingRootClientExt: ThinkingRootClient {}

impl<T: ThinkingRootClient + ?Sized> ThinkingRootClientExt for T {}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::openai_responses::{OpenAiFunctionCall, OpenAiResponse, OpenAiTokenUsage};
    use crate::types::{
        RetrievalMode, SourceMetadata, SourceType, ThinkingRootCaptureResult, ThinkingRootClaim,
        ThinkingRootFlowRun, ThinkingRootRoutedTool,
    };

    #[derive(Clone)]
    struct StubThinkingRootClient {
        capsule: ThinkingRootCapsule,
        route: Vec<ThinkingRootRoutedTool>,
        stored: Arc<Mutex<Vec<PostTurnKnowledgeCapture>>>,
        store_error: Option<String>,
        branch_events: Arc<Mutex<Vec<String>>>,
        merge_error: Option<String>,
        flow_run: Option<ThinkingRootFlowRun>,
        root_function_result: Value,
    }

    #[async_trait]
    impl ThinkingRootClient for StubThinkingRootClient {
        async fn fork_branch(
            &self,
            branch_id: String,
            parent: String,
            _description: Option<String>,
            _merge_policy: Option<String>,
            _identity: AgentIdentity,
        ) -> Result<(), String> {
            self.branch_events
                .lock()
                .unwrap()
                .push(format!("fork:{branch_id}:{parent}"));
            Ok(())
        }

        async fn checkout_branch(
            &self,
            branch_id: String,
            _identity: AgentIdentity,
        ) -> Result<(), String> {
            self.branch_events
                .lock()
                .unwrap()
                .push(format!("checkout:{branch_id}"));
            Ok(())
        }

        async fn capsule(
            &self,
            _request: PreparedTurnRequest,
            _identity: AgentIdentity,
        ) -> Result<ThinkingRootCapsule, String> {
            Ok(self.capsule.clone())
        }

        async fn route(
            &self,
            _query: String,
            _identity: AgentIdentity,
            _branch_id: Option<String>,
            _top_k: usize,
        ) -> Result<Vec<ThinkingRootRoutedTool>, String> {
            Ok(self.route.clone())
        }

        async fn store_scoped(
            &self,
            capture: PostTurnKnowledgeCapture,
        ) -> Result<ThinkingRootCaptureResult, String> {
            self.stored.lock().unwrap().push(capture);
            if let Some(error) = self.store_error.as_ref() {
                return Err(error.clone());
            }
            Ok(ThinkingRootCaptureResult { accepted: 1 })
        }

        async fn run_flow(
            &self,
            flow_id: String,
            _inputs: Value,
            _identity: AgentIdentity,
        ) -> Result<ThinkingRootFlowRun, String> {
            let mut flow_run = self
                .flow_run
                .clone()
                .ok_or_else(|| "no stub flow run configured".to_string())?;
            flow_run.flow_id = flow_id;
            Ok(flow_run)
        }

        async fn flow_run(
            &self,
            flow_id: String,
            flow_run_id: String,
            _identity: AgentIdentity,
        ) -> Result<ThinkingRootFlowRun, String> {
            let mut flow_run = self
                .flow_run
                .clone()
                .ok_or_else(|| "no stub flow run configured".to_string())?;
            flow_run.flow_id = flow_id;
            flow_run.flow_run_id = flow_run_id;
            Ok(flow_run)
        }

        async fn invoke_root_function(
            &self,
            function_name: String,
            input: Value,
            _identity: AgentIdentity,
        ) -> Result<Value, String> {
            Ok(json!({
                "function_name": function_name,
                "input": input,
                "result": self.root_function_result.clone(),
            }))
        }

        async fn merge_branch(
            &self,
            branch_id: String,
            _merge_policy: Option<String>,
            _identity: AgentIdentity,
        ) -> Result<(), String> {
            self.branch_events
                .lock()
                .unwrap()
                .push(format!("merge:{branch_id}"));
            if let Some(error) = self.merge_error.as_ref() {
                return Err(error.clone());
            }
            Ok(())
        }
    }

    struct StubOpenAiClient {
        responses: Arc<Mutex<VecDeque<OpenAiResponse>>>,
    }

    #[async_trait]
    impl OpenAiResponsesClient for StubOpenAiClient {
        async fn create_response(
            &self,
            _request: OpenAiResponseRequest,
        ) -> Result<OpenAiResponse, String> {
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| "no stub response left".to_string())
        }
    }

    fn prepared_turn_request() -> PreparedTurnRequest {
        PreparedTurnRequest {
            request_id: uuid::Uuid::new_v4(),
            query_text: "Check whether the server is listening.".to_string(),
            session_key: "cli:default:repo".to_string(),
            workspace_hint: Some("ownpager".to_string()),
            source: SourceMetadata {
                source_type: SourceType::Cli,
                user_id: Some("default".to_string()),
                chat_id: None,
                message_id: None,
                cwd: Some("/Users/alenjosephjohn/Multi/ownpager".to_string()),
            },
            retrieval_mode: RetrievalMode::PreTurn,
            branch_id: None,
            user_profile_id: None,
        }
    }

    fn agent_identity() -> AgentIdentity {
        AgentIdentity {
            agent_id: "ownpager".to_string(),
            scoped_user_id: "ownpager".to_string(),
            workspace: "ownpager".to_string(),
            prompt_name: Some("main".to_string()),
            session_id: "cli:default:repo".to_string(),
        }
    }

    fn capsule_with_tools(routed_tools: Vec<&str>, claims: Vec<&str>) -> ThinkingRootCapsule {
        ThinkingRootCapsule {
            system_prompt: "Use ThinkingRoot capsule context.".to_string(),
            grounded_claims: claims
                .into_iter()
                .enumerate()
                .map(|(index, statement)| ThinkingRootClaim {
                    claim_id: format!("claim-{index}"),
                    statement: statement.to_string(),
                    claim_type: "memory".to_string(),
                    source_uri: "memory://test".to_string(),
                })
                .collect(),
            routed_tools: routed_tools.into_iter().map(str::to_string).collect(),
            token_estimate: 74,
            query_class: Some("ops".to_string()),
            cache_hit: true,
            frame_warm: true,
            warnings: vec![],
        }
    }

    fn agent_config() -> AgentRuntimeConfig {
        AgentRuntimeConfig {
            max_tool_iterations: 2,
            terminal_timeout_seconds: 5,
            terminal_max_output_bytes: 4096,
            allow_local_terminal: true,
            routed_tool_top_k: 8,
            branch_per_run: true,
            branch_parent: "main".to_string(),
            branch_merge_policy: Some("verify-before-merge".to_string()),
            flow_id: None,
            flow_mode: FlowMode::Auto,
            flow_poll_interval_ms: 1,
            flow_timeout_seconds: 5,
        }
    }

    fn stub_thinkingroot() -> StubThinkingRootClient {
        StubThinkingRootClient {
            capsule: capsule_with_tools(vec![], vec![]),
            route: vec![],
            stored: Arc::new(Mutex::new(Vec::new())),
            store_error: None,
            branch_events: Arc::new(Mutex::new(Vec::new())),
            merge_error: None,
            flow_run: None,
            root_function_result: json!({"ok": true}),
        }
    }

    #[tokio::test]
    async fn returns_direct_answer_without_tools() {
        let stored = Arc::new(Mutex::new(Vec::new()));
        let thinkingroot = StubThinkingRootClient {
            capsule: capsule_with_tools(
                vec![],
                vec!["The service usually listens on localhost:8080."],
            ),
            route: vec![],
            stored: stored.clone(),
            store_error: None,
            branch_events: Arc::new(Mutex::new(Vec::new())),
            merge_error: None,
            flow_run: None,
            root_function_result: json!({"ok": true}),
        };
        let openai = StubOpenAiClient {
            responses: Arc::new(Mutex::new(VecDeque::from([OpenAiResponse {
                output_text: "The service is expected on localhost:8080.".to_string(),
                output_items: vec![json!({
                    "type": "message",
                    "content": [{"type": "output_text", "text": "The service is expected on localhost:8080."}]
                })],
                function_calls: vec![],
                usage: None,
            }]))),
        };

        let result = run_agent_turn(
            AgentTurnRequest {
                prepared_turn_request: prepared_turn_request(),
                agent_identity: Some(agent_identity()),
                agent_id: None,
                prompt_name: None,
                session_id: None,
            },
            &thinkingroot,
            &openai,
            &agent_config(),
            &OpenAiRuntimeConfig {
                api_key: "test".to_string(),
                base_url: "https://api.openai.com/v1".to_string(),
                model: "gpt-4.1-mini".to_string(),
                timeout_seconds: 30,
            },
        )
        .await
        .unwrap();

        assert_eq!(
            result.final_answer,
            "The service is expected on localhost:8080."
        );
        assert!(result.execution_plan.available_tool_names.is_empty());
        assert_eq!(stored.lock().unwrap().len(), 1);
        assert!(result.thinkingroot_hot_path.branch_run.unwrap().merged);
        assert_eq!(result.usage_estimate.api_call_count, 1);
        assert_eq!(result.usage_estimate.capsule_token_estimate, 74);
    }

    #[tokio::test]
    async fn uses_routed_terminal_then_answers() {
        let stored = Arc::new(Mutex::new(Vec::new()));
        let thinkingroot = StubThinkingRootClient {
            capsule: capsule_with_tools(vec!["function::terminal"], vec![]),
            route: vec![],
            stored,
            store_error: None,
            branch_events: Arc::new(Mutex::new(Vec::new())),
            merge_error: None,
            flow_run: None,
            root_function_result: json!({"ok": true}),
        };
        let openai = StubOpenAiClient {
            responses: Arc::new(Mutex::new(VecDeque::from([
                OpenAiResponse {
                    output_text: String::new(),
                    output_items: vec![json!({
                        "type": "function_call",
                        "call_id": "call_1",
                        "name": "terminal",
                        "arguments": "{\"command\":\"pwd\"}"
                    })],
                    function_calls: vec![OpenAiFunctionCall {
                        call_id: "call_1".to_string(),
                        name: "terminal".to_string(),
                        arguments: "{\"command\":\"pwd\"}".to_string(),
                    }],
                    usage: Some(OpenAiTokenUsage {
                        input_tokens: 80,
                        output_tokens: 12,
                        cached_input_tokens: 20,
                        reasoning_output_tokens: 0,
                        total_tokens: 112,
                    }),
                },
                OpenAiResponse {
                    output_text: "I checked the workspace and confirmed the current directory."
                        .to_string(),
                    output_items: vec![json!({
                        "type": "message",
                        "content": [{"type": "output_text", "text": "I checked the workspace and confirmed the current directory."}]
                    })],
                    function_calls: vec![],
                    usage: Some(OpenAiTokenUsage {
                        input_tokens: 120,
                        output_tokens: 18,
                        cached_input_tokens: 30,
                        reasoning_output_tokens: 4,
                        total_tokens: 168,
                    }),
                },
            ]))),
        };

        let result = run_agent_turn(
            AgentTurnRequest {
                prepared_turn_request: prepared_turn_request(),
                agent_identity: Some(agent_identity()),
                agent_id: None,
                prompt_name: None,
                session_id: None,
            },
            &thinkingroot,
            &openai,
            &agent_config(),
            &OpenAiRuntimeConfig {
                api_key: "test".to_string(),
                base_url: "https://api.openai.com/v1".to_string(),
                model: "gpt-4.1-mini".to_string(),
                timeout_seconds: 30,
            },
        )
        .await
        .unwrap();

        assert_eq!(result.execution_plan.available_tool_names, vec!["terminal"]);
        assert_eq!(result.tool_calls.len(), 1);
        assert!(result.tool_calls[0].routed);
        assert_eq!(result.usage_estimate.api_call_count, 2);
        assert_eq!(result.usage_estimate.input_tokens, 200);
        assert_eq!(result.usage_estimate.output_tokens, 30);
        assert_eq!(result.usage_estimate.cached_input_tokens, 50);
        assert_eq!(result.usage_estimate.reasoning_output_tokens, 4);
        assert_eq!(result.usage_estimate.total_tokens, 280);
    }

    #[tokio::test]
    async fn returns_partial_answer_when_tool_iteration_limit_is_reached() {
        let stored = Arc::new(Mutex::new(Vec::new()));
        let thinkingroot = StubThinkingRootClient {
            capsule: capsule_with_tools(vec!["function::terminal"], vec![]),
            route: vec![],
            stored,
            store_error: None,
            branch_events: Arc::new(Mutex::new(Vec::new())),
            merge_error: None,
            flow_run: None,
            root_function_result: json!({"ok": true}),
        };
        let repeated_tool_response = OpenAiResponse {
            output_text: String::new(),
            output_items: vec![json!({
                "type": "function_call",
                "call_id": "call_1",
                "name": "terminal",
                "arguments": "{\"command\":\"pwd\"}"
            })],
            function_calls: vec![OpenAiFunctionCall {
                call_id: "call_1".to_string(),
                name: "terminal".to_string(),
                arguments: "{\"command\":\"pwd\"}".to_string(),
            }],
            usage: Some(OpenAiTokenUsage {
                input_tokens: 10,
                output_tokens: 2,
                cached_input_tokens: 0,
                reasoning_output_tokens: 0,
                total_tokens: 12,
            }),
        };
        let openai = StubOpenAiClient {
            responses: Arc::new(Mutex::new(VecDeque::from([
                repeated_tool_response.clone(),
                repeated_tool_response.clone(),
                repeated_tool_response,
            ]))),
        };

        let result = run_agent_turn(
            AgentTurnRequest {
                prepared_turn_request: prepared_turn_request(),
                agent_identity: Some(agent_identity()),
                agent_id: None,
                prompt_name: None,
                session_id: None,
            },
            &thinkingroot,
            &openai,
            &agent_config(),
            &OpenAiRuntimeConfig {
                api_key: "test".to_string(),
                base_url: "https://api.openai.com/v1".to_string(),
                model: "gpt-4.1-mini".to_string(),
                timeout_seconds: 30,
            },
        )
        .await
        .unwrap();

        assert!(result
            .final_answer
            .contains("Stopped after reaching the max tool iteration limit"));
        assert_eq!(result.tool_calls.len(), 2);
        assert_eq!(result.usage_estimate.api_call_count, 3);
        assert_eq!(result.usage_estimate.total_tokens, 36);
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.contains("max tool iteration limit")));
    }

    #[tokio::test]
    async fn cold_start_falls_back_to_local_bootstrap_tool() {
        let stored = Arc::new(Mutex::new(Vec::new()));
        let thinkingroot = StubThinkingRootClient {
            capsule: ThinkingRootCapsule {
                system_prompt: String::new(),
                grounded_claims: vec![],
                routed_tools: vec![],
                token_estimate: 0,
                query_class: None,
                cache_hit: false,
                frame_warm: false,
                warnings: vec![],
            },
            route: vec![],
            stored,
            store_error: None,
            branch_events: Arc::new(Mutex::new(Vec::new())),
            merge_error: None,
            flow_run: None,
            root_function_result: json!({"ok": true}),
        };
        let openai = StubOpenAiClient {
            responses: Arc::new(Mutex::new(VecDeque::from([OpenAiResponse {
                output_text: "I need to inspect the workspace first.".to_string(),
                output_items: vec![json!({
                    "type": "message",
                    "content": [{"type": "output_text", "text": "I need to inspect the workspace first."}]
                })],
                function_calls: vec![],
                usage: None,
            }]))),
        };

        let result = run_agent_turn(
            AgentTurnRequest {
                prepared_turn_request: prepared_turn_request(),
                agent_identity: Some(agent_identity()),
                agent_id: None,
                prompt_name: None,
                session_id: None,
            },
            &thinkingroot,
            &openai,
            &agent_config(),
            &OpenAiRuntimeConfig {
                api_key: "test".to_string(),
                base_url: "https://api.openai.com/v1".to_string(),
                model: "gpt-4.1-mini".to_string(),
                timeout_seconds: 30,
            },
        )
        .await
        .unwrap();

        assert!(result.thinkingroot_hot_path.cold_start_degraded);
        assert!(result.execution_plan.used_local_fallback);
        assert_eq!(result.execution_plan.available_tool_names, vec!["terminal"]);
        assert_eq!(result.usage_estimate.api_call_count, 1);
    }

    #[tokio::test]
    async fn terminal_inspection_request_falls_back_to_local_tool_with_capsule_prompt() {
        let stored = Arc::new(Mutex::new(Vec::new()));
        let thinkingroot = StubThinkingRootClient {
            capsule: capsule_with_tools(vec![], vec![]),
            route: vec![],
            stored,
            store_error: None,
            branch_events: Arc::new(Mutex::new(Vec::new())),
            merge_error: None,
            flow_run: None,
            root_function_result: json!({"ok": true}),
        };
        let openai = StubOpenAiClient {
            responses: Arc::new(Mutex::new(VecDeque::from([OpenAiResponse {
                output_text: "I can inspect the workspace with the terminal.".to_string(),
                output_items: vec![json!({
                    "type": "message",
                    "content": [{"type": "output_text", "text": "I can inspect the workspace with the terminal."}]
                })],
                function_calls: vec![],
                usage: None,
            }]))),
        };
        let mut request = prepared_turn_request();
        request.query_text =
            "Use the available terminal tooling to inspect the workspace directory.".to_string();

        let result = run_agent_turn(
            AgentTurnRequest {
                prepared_turn_request: request,
                agent_identity: Some(agent_identity()),
                agent_id: None,
                prompt_name: None,
                session_id: None,
            },
            &thinkingroot,
            &openai,
            &agent_config(),
            &OpenAiRuntimeConfig {
                api_key: "test".to_string(),
                base_url: "https://api.openai.com/v1".to_string(),
                model: "gpt-4.1-mini".to_string(),
                timeout_seconds: 30,
            },
        )
        .await
        .unwrap();

        assert!(!result.thinkingroot_hot_path.cold_start_degraded);
        assert!(result.execution_plan.used_local_fallback);
        assert_eq!(result.execution_plan.available_tool_names, vec!["terminal"]);
        assert_eq!(result.usage_estimate.api_call_count, 1);
    }

    #[tokio::test]
    async fn complex_task_runs_thinkingroot_flow() {
        let stored = Arc::new(Mutex::new(Vec::new()));
        let thinkingroot = StubThinkingRootClient {
            capsule: capsule_with_tools(vec![], vec!["Competitor research belongs in a flow."]),
            route: vec![],
            stored: stored.clone(),
            store_error: None,
            branch_events: Arc::new(Mutex::new(Vec::new())),
            merge_error: None,
            flow_run: Some(ThinkingRootFlowRun {
                flow_id: "demo-flow".to_string(),
                flow_run_id: "flow-run-1".to_string(),
                status: "completed".to_string(),
                output: Some(json!({
                    "final_answer": "Research complete. Report drafted."
                })),
                raw: json!({"status": "completed"}),
            }),
            root_function_result: json!({"ok": true}),
        };
        let openai = StubOpenAiClient {
            responses: Arc::new(Mutex::new(VecDeque::new())),
        };
        let mut config = agent_config();
        config.flow_id = Some("demo-flow".to_string());
        config.flow_mode = FlowMode::Auto;
        let mut request = prepared_turn_request();
        request.query_text =
            "Research competitors, then write a report, then email it.".to_string();

        let result = run_agent_turn(
            AgentTurnRequest {
                prepared_turn_request: request,
                agent_identity: Some(agent_identity()),
                agent_id: None,
                prompt_name: None,
                session_id: None,
            },
            &thinkingroot,
            &openai,
            &config,
            &OpenAiRuntimeConfig {
                api_key: "test".to_string(),
                base_url: "https://api.openai.com/v1".to_string(),
                model: "gpt-4.1-mini".to_string(),
                timeout_seconds: 30,
            },
        )
        .await
        .unwrap();

        assert_eq!(result.final_answer, "Research complete. Report drafted.");
        assert_eq!(
            result
                .flow_run
                .as_ref()
                .map(|flow| flow.flow_run_id.as_str()),
            Some("flow-run-1")
        );
        assert!(result.execution_plan.available_tool_names.is_empty());
        assert_eq!(stored.lock().unwrap().len(), 1);
        assert_eq!(result.usage_estimate.api_call_count, 0);
        assert_eq!(result.usage_estimate.capsule_token_estimate, 74);
    }

    #[tokio::test]
    async fn unrouted_tool_is_blocked() {
        let registry = ToolRegistry::default();
        let thinkingroot = stub_thinkingroot();
        let plan = registry.build_execution_plan(&[], false, &agent_config());
        let outcome = registry
            .execute(
                &plan,
                "terminal",
                "{\"command\":\"pwd\"}",
                &prepared_turn_request(),
                &thinkingroot,
                &agent_identity(),
                5,
                4096,
            )
            .await
            .unwrap();

        assert_eq!(
            outcome.result.get("error").and_then(Value::as_str),
            Some("tool `terminal` was not routed for this turn")
        );
        assert!(!outcome.routed);
    }

    #[tokio::test]
    async fn routed_unknown_tool_invokes_root_function() {
        let registry = ToolRegistry::default();
        let thinkingroot = stub_thinkingroot();
        let plan = registry.build_execution_plan(
            &["function::sendgrid".to_string()],
            false,
            &agent_config(),
        );
        let tool_name = openai_root_function_tool_name("function::sendgrid");
        let outcome = registry
            .execute(
                &plan,
                &tool_name,
                "{\"input\":{\"to\":\"judge@example.com\"}}",
                &prepared_turn_request(),
                &thinkingroot,
                &agent_identity(),
                5,
                4096,
            )
            .await
            .unwrap();

        assert_eq!(
            outcome.result.get("function_name").and_then(Value::as_str),
            Some("function::sendgrid")
        );
        assert_eq!(outcome.tool_name, "function::sendgrid");
        assert!(outcome.routed);
    }

    #[tokio::test]
    async fn store_failure_does_not_fail_turn() {
        let stored = Arc::new(Mutex::new(Vec::new()));
        let thinkingroot = StubThinkingRootClient {
            capsule: capsule_with_tools(
                vec![],
                vec!["The service usually listens on localhost:8080."],
            ),
            route: vec![],
            stored,
            store_error: Some("write failed".to_string()),
            branch_events: Arc::new(Mutex::new(Vec::new())),
            merge_error: None,
            flow_run: None,
            root_function_result: json!({"ok": true}),
        };
        let openai = StubOpenAiClient {
            responses: Arc::new(Mutex::new(VecDeque::from([OpenAiResponse {
                output_text: "The service is expected on localhost:8080.".to_string(),
                output_items: vec![json!({
                    "type": "message",
                    "content": [{"type": "output_text", "text": "The service is expected on localhost:8080."}]
                })],
                function_calls: vec![],
                usage: None,
            }]))),
        };

        let result = run_agent_turn(
            AgentTurnRequest {
                prepared_turn_request: prepared_turn_request(),
                agent_identity: Some(agent_identity()),
                agent_id: None,
                prompt_name: None,
                session_id: None,
            },
            &thinkingroot,
            &openai,
            &agent_config(),
            &OpenAiRuntimeConfig {
                api_key: "test".to_string(),
                base_url: "https://api.openai.com/v1".to_string(),
                model: "gpt-4.1-mini".to_string(),
                timeout_seconds: 30,
            },
        )
        .await
        .unwrap();

        assert!(result.capture_result.is_none());
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.contains("ThinkingRoot scoped store failed")));
    }
}
