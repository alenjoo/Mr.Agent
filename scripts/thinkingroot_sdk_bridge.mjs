import { stdin, stderr, stdout } from "node:process";
import { HttpError, thinkingroot } from "@thinkingroot/sdk";

const action = process.argv[2];

if (!action) {
  fail("missing action");
}

const input = await readStdin();
const payload = input.trim() ? JSON.parse(input) : {};

switch (action) {
  case "fork_branch":
    await handleForkBranch(payload);
    break;
  case "checkout_branch":
    await handleCheckoutBranch(payload);
    break;
  case "merge_branch":
    await handleMergeBranch(payload);
    break;
  case "capsule":
    await handleCapsule(payload);
    break;
  case "route":
    await handleRoute(payload);
    break;
  case "store":
    await handleStore(payload);
    break;
  case "run_flow":
    await handleRunFlow(payload);
    break;
  case "flow_run":
    await handleFlowRun(payload);
    break;
  case "invoke_function":
    await handleInvokeFunction(payload);
    break;
  default:
    fail(`unsupported action: ${action}`);
}

async function handleCapsule(payload) {
  const { envelope, query, branch_id } = payload;
  const client = buildClient(envelope);
  const scoped = client.scope(envelope.scoped_user_id);
  const warnings = [];

  if (branch_id) {
    warnings.push(
      `ThinkingRoot branch preference requested (${branch_id}) but this V3 hot path uses scoped agent brains rather than branch-targeted capsule reads.`
    );
  }

  const capsule = await scoped.capsule({
    promptName: envelope.prompt_name,
    query,
    topK: envelope.top_k,
    maxTools: envelope.max_tools,
    sessionId: envelope.session_id,
    vars: {
      workspace: envelope.workspace,
      session_id: envelope.session_id,
      scoped_user_id: envelope.scoped_user_id,
    },
  });

  stdout.write(
    `${JSON.stringify({
      system_prompt: capsule.system ?? "",
      grounded_claims: normalizeClaims(capsule.grounded_claims),
      routed_tools: Array.isArray(capsule.tools) ? capsule.tools.map(String) : [],
      token_estimate: Number(capsule.token_estimate ?? 0),
      query_class:
        capsule.query_class === undefined || capsule.query_class === null
          ? null
          : String(capsule.query_class),
      cache_hit: capsule.cache_hit === true,
      frame_warm: capsule.frame_warm === true,
      warnings,
    })}\n`
  );
}

async function handleRoute(payload) {
  const { envelope, query, branch_id, top_k } = payload;
  const result = await postWorkspaceJson(
    envelope,
    `u_${envelope.scoped_user_id}`,
    "/route",
    {
      query,
      ...(branch_id ? { branch: branch_id } : {}),
      ...(top_k !== undefined ? { top_k } : {}),
    },
    { "x-tr-user": envelope.scoped_user_id }
  );
  const ranked = Array.isArray(result?.ranked) ? result.ranked : [];

  stdout.write(
    `${JSON.stringify(
      ranked.map((tool) => ({
        name: String(tool.name ?? ""),
        score:
          typeof tool.score === "number" && Number.isFinite(tool.score)
            ? tool.score
            : null,
      }))
    )}\n`
  );
}

async function handleWorkspaceRoute(payload) {
  const { envelope, query, branch_id, top_k } = payload;
  const workspace = buildClient(envelope).workspace(envelope.workspace);
  return workspace.route(query, {
    topK: top_k,
    ...(branch_id ? { branch: branch_id } : {}),
  });
}

async function postWorkspaceJson(envelope, workspace, suffix, body, extraHeaders = {}) {
  if (!envelope?.project_key) {
    fail("THINKINGROOT_PROJECT_KEY or THINKINGROOT_API_KEY is required");
  }

  const base = String(envelope.gateway_url ?? "").replace(/\/$/, "");
  const response = await fetch(
    `${base}/engine/api/v1/ws/${encodeURIComponent(workspace)}${suffix}`,
    {
      method: "POST",
      headers: {
        authorization: `Bearer ${envelope.project_key}`,
        "content-type": "application/json",
        ...extraHeaders,
      },
      body: JSON.stringify(body ?? {}),
    }
  );
  const text = await response.text();
  if (!response.ok) {
    throw new HttpError(response.status, text);
  }
  const parsed = text ? JSON.parse(text) : {};
  return parsed && typeof parsed === "object" && "data" in parsed
    ? parsed.data
    : parsed;
}

async function handleForkBranch(payload) {
  const { envelope, branch_id, parent, description, merge_policy } = payload;
  const workspace = buildClient(envelope).workspace(envelope.workspace);
  const existing = await branchExists(workspace, branch_id);

  if (!existing) {
    await workspace.forkBranch(branch_id, parent ?? "main", {
      ...(description ? { description } : {}),
      ...(merge_policy ? { mergePolicy: merge_policy } : {}),
    });
  }

  stdout.write(
    `${JSON.stringify({
      ok: true,
      branch_id,
      parent: parent ?? "main",
      existed: existing,
    })}\n`
  );
}

async function handleCheckoutBranch(payload) {
  const { envelope, branch_id } = payload;
  const workspace = buildClient(envelope).workspace(envelope.workspace);
  await workspace.checkoutBranch(branch_id);
  stdout.write(`${JSON.stringify({ ok: true, branch_id })}\n`);
}

async function handleMergeBranch(payload) {
  const { envelope, branch_id, merge_policy } = payload;
  const workspace = buildClient(envelope).workspace(envelope.workspace);
  const result = await workspace.mergeBranch(branch_id, merge_policy ?? undefined);
  stdout.write(
    `${JSON.stringify({ ok: result?.ok === undefined ? true : Boolean(result.ok), branch_id })}\n`
  );
}

async function handleStore(payload) {
  const { envelope, capture } = payload;
  const client = buildClient(envelope);
  const scoped = client.scope(envelope.scoped_user_id);
  const facts = Array.isArray(capture?.facts) ? capture.facts : [];
  const decisions = Array.isArray(capture?.decisions) ? capture.decisions : [];
  const claims = [
    ...facts.map((statement) => ({ statement, claim_type: "memory" })),
    ...decisions.map((statement) => ({ statement, claim_type: "decision" })),
  ];

  if (!claims.length) {
    stdout.write(`${JSON.stringify({ accepted: 0 })}\n`);
    return;
  }

  const result = await scoped.store(
    claims,
    capture?.request_id
      ? `ownpager-${capture.request_id}`
      : `ownpager-${envelope.scoped_user_id}-${claims.length}`
  );

  stdout.write(
    `${JSON.stringify({ accepted: Number(result.accepted_count ?? 0) })}\n`
  );
}

async function handleRunFlow(payload) {
  const { envelope, flow_id, inputs } = payload;
  const workspace = buildClient(envelope).workspace(envelope.workspace);
  const handle = await workspace.runFlow(flow_id, inputs ?? {});
  stdout.write(`${JSON.stringify(normalizeFlowRun(flow_id, handle))}\n`);
}

async function handleFlowRun(payload) {
  const { envelope, flow_id, flow_run_id } = payload;
  const workspace = buildClient(envelope).workspace(envelope.workspace);
  const snapshot = await workspace.flowRun(flow_run_id);
  stdout.write(`${JSON.stringify(normalizeFlowRun(flow_id, snapshot))}\n`);
}

async function handleInvokeFunction(payload) {
  const { envelope, function_name, input } = payload;
  const workspace = buildClient(envelope).workspace(envelope.workspace);
  const result = await workspace.invokeFunction(function_name, input ?? {});
  stdout.write(`${JSON.stringify(result?.result ?? result ?? null)}\n`);
}

function buildClient(envelope) {
  if (!envelope?.project_key) {
    fail("THINKINGROOT_PROJECT_KEY or THINKINGROOT_API_KEY is required");
  }

  return thinkingroot({
    gatewayUrl: envelope.gateway_url,
    projectKey: envelope.project_key,
  });
}

function normalizeClaims(claims) {
  if (!Array.isArray(claims)) {
    return [];
  }

  return claims.map((claim) => ({
    claim_id: String(claim.claim_id ?? ""),
    statement: String(claim.statement ?? ""),
    claim_type: String(claim.claim_type ?? ""),
    source_uri: String(claim.source_uri ?? ""),
  }));
}

async function branchExists(workspace, branchId) {
  const branches = await workspace.branches();
  if (!Array.isArray(branches)) {
    return false;
  }
  return branches.some((branch) => String(branch?.name ?? "") === branchId);
}

function normalizeFlowRun(flowId, value) {
  const flowRunId = String(
    value?.flow_run_id ?? value?.run_id ?? value?.id ?? ""
  );
  const status = String(value?.status ?? "unknown");
  const output =
    value?.output ?? value?.final ?? value?.result ?? value?.output_json ?? null;

  return {
    flow_id: flowId,
    flow_run_id: flowRunId,
    status,
    output,
    raw: value ?? {},
  };
}

async function readStdin() {
  const chunks = [];
  for await (const chunk of stdin) {
    chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk));
  }
  return Buffer.concat(chunks).toString("utf8");
}

function fail(message) {
  stderr.write(`${message}\n`);
  process.exit(1);
}
