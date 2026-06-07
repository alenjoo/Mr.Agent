"use strict";
const app = document.querySelector("#app");
if (!app) {
    throw new Error("App root not found");
}
const sampleRequirements = [
    "Research three competitors, compare their onboarding flows, then draft a concise launch plan.",
    "Review the OwnPager architecture and suggest the safest next refactor.",
    "Design a Telegram-first capture workflow for a multi-agent founder assistant.",
];
const webSessionId = getWebSessionId();
const state = {
    turns: [],
    selectedMode: "run",
};
app.innerHTML = `
  <section class="app-layout" aria-label="OwnPager requirement interface">
    <aside class="control-rail">
      <section class="rail-panel">
        <div class="rail-heading">
          <span class="eyebrow">Runtime</span>
          <h2>OwnPager</h2>
        </div>
        <div class="runtime-stack">
          <div class="runtime-row">
            <span>Branching</span>
            <strong>Run lifecycle</strong>
          </div>
          <div class="runtime-row">
            <span>Prompts</span>
            <strong>Compiled</strong>
          </div>
          <div class="runtime-row">
            <span>Tools</span>
            <strong>Root Functions</strong>
          </div>
          <div class="runtime-row">
            <span>Flows</span>
            <strong>Auto</strong>
          </div>
        </div>
      </section>

    </aside>

    <section class="workbench">
      <div class="workbench-top">
        <div>
          <h2>Tell the swarm what to do.</h2>
        </div>
        <div class="status-pill" data-connection-status>
          <span></span>
          Web source
        </div>
      </div>

      <div class="conversation" data-conversation>
      </div>

      <form class="composer" data-composer>
        <div class="mode-tabs" style="display: none;" role="tablist" aria-label="Turn mode">
          <button type="button" class="mode-tab is-active" data-mode="run">Run</button>
        </div>

        <label class="requirement-box">
          <span>Requirement</span>
          <textarea
            name="requirement"
            data-requirement
            rows="6"
            placeholder="Example: Research competitors, then write a report, then prepare a Telegram summary."
          ></textarea>
        </label>

        <div class="composer-footer">
          <button class="send-button" type="submit" data-submit-button>
            <span>Send turn</span>
            <strong>Go</strong>
          </button>
        </div>
      </form>
    </section>

    <aside class="detail-panel">
      <section class="detail-card">
        <div class="rail-heading compact">
          <span class="eyebrow">Turn Path</span>
          <h3>Execution plan</h3>
        </div>
        <ol class="path-list" data-path-list>
          <li class="is-current">Capture requirement</li>
          <li>Fork branch lifecycle</li>
          <li>Compile scoped capsule</li>
          <li>Route Flow or Root Functions</li>
          <li>Store memory and merge</li>
        </ol>
      </section>

      <section class="detail-card token-card">
        <div class="rail-heading compact">
          <span class="eyebrow">Tokens</span>
          <h3>Response usage</h3>
        </div>
        <div class="token-panel" data-token-panel></div>
      </section>

    </aside>
  </section>
`;
const composer = document.querySelector("[data-composer]");
const conversation = document.querySelector("[data-conversation]");
const sessionList = document.querySelector("[data-session-list]") || document.createElement("div");
const commandOutput = document.querySelector("[data-command-output]") || document.createElement("div");
const pathList = document.querySelector("[data-path-list]");
const tokenPanel = document.querySelector("[data-token-panel]");
const requirementInput = document.querySelector("[data-requirement]");
const submitButton = document.querySelector("[data-submit-button]");
const modeTabs = Array.from(document.querySelectorAll("[data-mode]"));
if (!composer || !conversation || !pathList || !tokenPanel || !requirementInput || !submitButton) {
    throw new Error("OwnPager interface failed to mount");
}
const conversationEl = conversation;
const sessionListEl = sessionList;
const commandOutputEl = commandOutput;
const pathListEl = pathList;
const tokenPanelEl = tokenPanel;
const submitButtonEl = submitButton;
modeTabs.forEach((button) => {
    button.addEventListener("click", () => {
        state.selectedMode = button.dataset.mode;
        modeTabs.forEach((tab) => tab.classList.toggle("is-active", tab === button));
        updatePath();
    });
});
document.querySelectorAll("[data-sample]").forEach((button) => {
    button.addEventListener("click", () => {
        const index = Number(button.dataset.sample ?? 0);
        requirementInput.value = sampleRequirements[index] ?? sampleRequirements[0];
        requirementInput.focus();
    });
});
const urlRequirement = new URLSearchParams(window.location.search).get("requirement");
if (urlRequirement && !requirementInput.value.trim()) {
    requirementInput.value = urlRequirement;
}
composer.addEventListener("submit", async (event) => {
    event.preventDefault();
    const form = new FormData(composer);
    const requirement = String(form.get("requirement") ?? "").trim();
    const profile = String(form.get("profile") ?? "default").trim() || "default";
    if (!requirement) {
        requirementInput.focus();
        composer.classList.add("needs-input");
        window.setTimeout(() => composer.classList.remove("needs-input"), 500);
        return;
    }
    const turn = {
        id: crypto.randomUUID(),
        requirement,
        mode: state.selectedMode,
        profile,
        createdAt: new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }),
        status: state.selectedMode === "preview" ? "prepared" : "running",
    };
    state.turns.unshift(turn);
    const assistantBody = renderTurn(turn);
    renderSessions();
    renderCommand(turn);
    updatePath(turn);
    renderTokenPanel();
    requirementInput.value = "";
    if (turn.mode === "preview") {
        return;
    }
    submitButtonEl.disabled = true;
    try {
        const result = await runWebTurn(turn);
        turn.status = "completed";
        turn.answer = String(result.final_answer ?? "");
        turn.usageEstimate = result.usage_estimate;
        updateAssistantTurn(assistantBody, turn);
        renderTokenPanel();
    }
    catch (error) {
        turn.status = "failed";
        turn.error = error instanceof Error ? error.message : String(error);
        updateAssistantTurn(assistantBody, turn);
        renderTokenPanel();
    }
    finally {
        submitButtonEl.disabled = false;
        renderSessions();
    }
});
function renderTurn(turn) {
    const userMessage = document.createElement("article");
    userMessage.className = "message user-message";
    userMessage.innerHTML = `
    <div class="message-avatar">YOU</div>
    <div class="message-body">
      <div class="message-meta">
        <strong>${escapeHtml(turn.profile)}</strong>
        <span>${escapeHtml(turn.createdAt)}</span>
      </div>
      <p>${escapeHtml(turn.requirement)}</p>
    </div>
  `;
    const assistantMessage = document.createElement("article");
    assistantMessage.className = "message assistant-message";
    assistantMessage.innerHTML = `
    <div class="message-avatar">OP</div>
    <div class="message-body">
      <div class="message-meta">
        <strong>${turn.mode === "preview" ? "Prepared turn" : "Running turn"}</strong>
        <span>${turn.status}</span>
      </div>
      <div class="turn-summary">
        <span>source: web</span>
        <span>config: .env</span>
        <span>${turn.mode === "flow" ? "ThinkingRoot Flow preferred" : turn.mode === "run" ? "Root Functions enabled" : "preview only"}</span>
      </div>
    </div>
  `;
    conversationEl.append(userMessage, assistantMessage);
    assistantMessage.scrollIntoView({ behavior: "smooth", block: "end" });
    return assistantMessage.querySelector(".message-body");
}
function renderSessions() {
    sessionListEl.innerHTML = state.turns.length
        ? state.turns
            .slice(0, 5)
            .map((turn) => `
            <button class="session-item" data-turn-id="${turn.id}">
              <strong>${escapeHtml(turn.requirement)}</strong>
              <span>${escapeHtml(turn.mode)} - ${escapeHtml(turn.status)} - ${escapeHtml(turn.createdAt)}</span>
            </button>
          `)
            .join("")
        : `<p class="empty-state">No submitted turns yet.</p>`;
}
function renderCommand(turn) {
    const command = turn.mode === "preview" ? "cli" : "run-cli";
    const env = [
        turn.mode === "flow" ? "OWNPAGER_FLOW_MODE=always" : "",
    ].filter(Boolean);
    const prefix = env.length ? `${env.join(" ")} ` : "";
    commandOutputEl.textContent = `${prefix}cargo run -- ${command} --profile ${shellQuote(turn.profile)} --message ${shellQuote(turn.requirement)}`;
}
async function runWebTurn(turn) {
    const body = JSON.stringify({
        message: turn.requirement,
        profile: turn.profile,
        client_session_id: webSessionId,
    });
    const bases = apiCandidates();
    let lastError = "";
    for (const base of bases) {
        try {
            const response = await fetch(`${base}/api/turn`, {
                method: "POST",
                headers: {
                    "content-type": "application/json",
                },
                body,
            });
            const text = await response.text();
            if (response.ok) {
                return text ? JSON.parse(text) : {};
            }
            lastError = text || `OwnPager web request failed with HTTP ${response.status}`;
            if (response.status !== 404 && response.status !== 405) {
                break;
            }
        }
        catch (error) {
            lastError = error instanceof Error ? error.message : String(error);
        }
    }
    throw new Error(lastError || "OwnPager web API is not reachable.");
}
function updateAssistantTurn(container, turn) {
    const meta = container.querySelector(".message-meta");
    if (meta) {
        meta.innerHTML = `
      <strong>${turn.status === "completed" ? "OwnPager result" : "OwnPager error"}</strong>
      <span>${escapeHtml(turn.status)}</span>
    `;
    }
    const content = turn.status === "completed"
        ? `<p>${escapeHtml(turn.answer ?? "")}</p>${renderUsageEstimate(turn.usageEstimate)}`
        : `<p>${escapeHtml(turn.error ?? "The web turn failed.")}</p>`;
    const summary = container.querySelector(".turn-summary");
    if (summary) {
        summary.outerHTML = content;
    }
    else {
        container.insertAdjacentHTML("beforeend", content);
    }
}
function renderUsageEstimate(usage) {
    if (!usage) {
        return "";
    }
    const chips = [
        ["API calls", usage.api_call_count],
        ["Capsule est.", usage.capsule_token_estimate],
        ["Input", usage.input_tokens],
        ["Cached", usage.cached_input_tokens],
        ["Output", usage.output_tokens],
        ["Reasoning", usage.reasoning_output_tokens],
        ["Total", usage.total_tokens],
    ]
        .filter(([, value]) => typeof value === "number")
        .map(([label, value]) => `<span><strong>${escapeHtml(String(label))}</strong>${formatNumber(Number(value))}</span>`)
        .join("");
    return chips ? `<div class="usage-summary">${chips}</div>` : "";
}
function renderTokenPanel() {
    const latest = state.turns[0];
    if (!latest) {
        tokenPanelEl.innerHTML = `<p class="empty-state">No response yet.</p>`;
        return;
    }
    if (latest.status === "running") {
        tokenPanelEl.innerHTML = `
      <div class="token-state">
        <strong>Running</strong>
        <span>Waiting for OpenAI usage</span>
      </div>
    `;
        return;
    }
    if (latest.status === "failed") {
        tokenPanelEl.innerHTML = `
      <div class="token-state failed">
        <strong>Failed</strong>
        <span>No token usage returned</span>
      </div>
    `;
        return;
    }
    const usage = latest.usageEstimate;
    if (!usage) {
        tokenPanelEl.innerHTML = `<p class="empty-state">No usage data.</p>`;
        return;
    }
    tokenPanelEl.innerHTML = `
    <div class="token-total">
      <span>Total tokens</span>
      <strong>${formatNumber(usage.total_tokens ?? 0)}</strong>
    </div>
    <div class="token-grid">
      ${renderTokenRow("API calls", usage.api_call_count)}
      ${renderTokenRow("Capsule", usage.capsule_token_estimate)}
      ${renderTokenRow("Input", usage.input_tokens)}
      ${renderTokenRow("Cached", usage.cached_input_tokens)}
      ${renderTokenRow("Output", usage.output_tokens)}
      ${renderTokenRow("Reasoning", usage.reasoning_output_tokens)}
    </div>
  `;
}
function renderTokenRow(label, value) {
    return `
    <div class="token-row">
      <span>${escapeHtml(label)}</span>
      <strong>${formatNumber(value ?? 0)}</strong>
    </div>
  `;
}
function formatNumber(value) {
    return new Intl.NumberFormat().format(value);
}
function getWebSessionId() {
    const key = "ownpager.webSessionId";
    const existing = window.localStorage.getItem(key);
    if (existing) {
        return existing;
    }
    const created = crypto.randomUUID();
    window.localStorage.setItem(key, created);
    return created;
}
function apiCandidates() {
    const host = window.location.hostname || "127.0.0.1";
    const protocol = window.location.protocol || "http:";
    const candidates = [
        ["8080", "8081", "8082"].includes(window.location.port)
            ? window.location.origin
            : "",
<<<<<<< HEAD
        `${protocol}//${host}:8082`,
        `${protocol}//${host}:8081`,
        `${protocol}//${host}:8080`,
=======
        `${protocol}//${host}:8080`,
        `${protocol}//${host}:8081`,
        `${protocol}//${host}:8082`,
>>>>>>> a4431edb3da484ff49672ad59b00295a092f0813
    ].filter(Boolean);
    return Array.from(new Set(candidates));
}
function updatePath(turn) {
    const labels = (turn?.mode ?? state.selectedMode) === "flow"
        ? [
            "Capture requirement",
            "Fork branch lifecycle",
            "Compile scoped capsule",
            "Run ThinkingRoot Flow",
            "Store memory and merge",
        ]
        : [
            "Capture requirement",
            "Fork branch lifecycle",
            "Compile scoped capsule",
            "Route Root Functions",
            "Store memory and merge",
        ];
    pathListEl.innerHTML = labels
        .map((label, index) => `<li class="${index === 0 ? "is-current" : ""}">${label}</li>`)
        .join("");
}
function shellQuote(value) {
    return `'${value.replace(/'/g, "'\\''")}'`;
}
function escapeHtml(value) {
    return value
        .replace(/&/g, "&amp;")
        .replace(/</g, "&lt;")
        .replace(/>/g, "&gt;")
        .replace(/"/g, "&quot;");
}
renderSessions();
updatePath();
renderTokenPanel();
