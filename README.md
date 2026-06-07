# OwnPager

OwnPager is an intelligent agent and requirement console application designed to execute complex tasks and manage autonomous agent workflows. It features a high-performance backend written in Rust and a minimalist web frontend. 

The system tightly integrates with [ThinkingRoot DB](https://github.com/DevbyNaveen/ThinkingRoot) for state management, flow control, and branch orchestration.

## Architecture overview

OwnPager employs a decoupled, high-performance architecture split across a Rust backend daemon, a TypeScript frontend, and a cloud-based state management layer. Conceptually, the architecture functions as a stateful, agentic query-processing pipeline that isolates reasoning paths and synchronizes knowledge.

### 1. Ingestion & Intake Layer
OwnPager acts as a multi-channel intake gateway. Requests are ingested from three distinct entry points:
* **CLI Console**: Running individual task runs via terminal commands.
* **Web Server API**: An Axum-based REST server that exposes endpoints (like `/api/turn`) to interact with browser-based tools.
* **Telegram Wrapper**: An asynchronous polling service that routes messaging channel updates to the agent.

Each query is processed to resolve session keys, user profiles, and active workspace paths before execution begins.

### 2. Context Retrieval & Branch Isolation
When a turn starts, the system isolates the execution workspace:
* **ThinkingRoot Branching**: To prevent context pollution and enable rollbacks, the agent forks a temporary, request-scoped branch in [ThinkingRoot DB](https://github.com/DevbyNaveen/ThinkingRoot).
* **Capsule Compilation**: The backend retrieves a compiled *Capsule* from ThinkingRoot. This capsule consolidates the dynamic system prompt, relevant grounded context claims (facts), and permitted tools.

### 3. Reasoning & Execution Loop
With the context compiled, the core execution engine runs the reasoning loop:
* **ThinkingRoot Flow Dispatch**: For complex, multi-step tasks, the agent dispatches execution to a ThinkingRoot Flow and polls for output completion.
* **Local Tool Execution**: For reasoning turns, the engine queries the LLM. If the LLM requests tool use (like a terminal command), the engine executes the shell command in a secured execution environment restricting timeouts, output sizes, and directories.
* **Iteration Resilience**: If the agent reaches its maximum tool iteration threshold, it halts gracefully and returns a partial answer based on gathered evidence rather than failing.

### 4. Knowledge Capture & Synchronization
Once a turn concludes, the agent commits the outcome:
* **Memory Capture**: Key facts and decisions from the turn are summarized.
* **Store & Merge**: The summarized knowledge is pushed back to the user scope in ThinkingRoot DB, and the temporary branch is merged back into the parent branch.
* **Stateless Backend**: Because the state is synchronized externally, the Rust daemon remains resilient to crashes and restarts without losing task progress.

## SDK Bridge Subsystem

To minimize duplication of the ThinkingRoot core API, the backend employs a hybrid Rust-to-Node subprocess bridge.
* **Bridge Execution**: The Rust daemon launches a Node.js process executing `scripts/thinkingroot_sdk_bridge.mjs`.
* **IPC Transport**: Structured requests are serialized into JSON envelopes and transmitted via stdin, with replies returned on stdout.
* **Bridge Actions**: Supported actions include branch creation, checkout, merge, capsule compilation, routing, and memory storage.

## CLI Subcommands Reference

The compiled Rust binary (`target/debug/ownpager`) provides direct commands for pipeline administration:
* `cli --message "<query>"`: Simulates turn ingestion and logs a JSON preview of the prepared turn and boundary variables.
* `run-cli --message "<query>"`: Spawns the agent runner loop to run the query, execute local terminal tasks, and sync state.
* `serve-web`: Launches the Axum web daemon to interface with local ports (such as `8080`, `8081`, `8082`) for frontend routing.
* `serve-telegram`: Runs a long-polling Telegram update receiver thread to pipe direct messages to the worker.
* `serve-telegram-run`: Starts the Telegram client and executes complete agent turns for every received update.
* `terminal --command "<cmd>"`: Invokes a sandboxed command runner with configured timeout, directory, and output restrictions.

## Dependencies

- **ThinkingRoot DB**: Central database for managing conversational states, workflow configurations, and memory capsules.
- **OpenAI API**: The core Large Language Model (LLM) engine for reasoning, task parsing, and tool-use generation.
- **Rust Toolchain**: Cargo compile runtime required to build the backend daemon and run check validations.
- **Node.js**: The Javascript execution engine required to spin up the ThinkingRoot SDK subprocess bridge.

## Configuration

Before starting the application, you must configure your environment variables. 

1. Copy `.env.example` to `.env`
2. Populate the required keys.

Key variables include:
- `OPENAI_API_KEY`: Your OpenAI access token.
- `THINKINGROOT_PROJECT_KEY`: Your project key from ThinkingRoot DB.
- `THINKINGROOT_WORKSPACE`: The targeted workspace.
- `OWNPAGER_FLOW_MODE`: Dictates the level of autonomy the agent has during execution.

## Getting started

### Backend Setup

1. Navigate to the project root.
2. Build the project using Cargo:
   ```bash
   cargo build --release
   ```
3. Run the compiled binary:
   ```bash
   cargo run
   ```

### Frontend Setup

1. Navigate to the `frontend` directory.
2. Install dependencies:
   ```bash
   npm install
   ```
3. Build the static assets:
   ```bash
   npm run build
   ```

## Development

During active development, it is recommended to run the frontend via the development server while running the Rust backend via `cargo run`.

## Troubleshooting

* **Refusing to Merge Unrelated Histories**: Resolved by pulling with `git pull origin main --allow-unrelated-histories`.
* **Leaked Credentials**: Run `git filter-branch --force --index-filter "git rm --cached --ignore-unmatch .env" --prune-empty --tag-name-filter cat -- --all` and force-push.
* **Bridge Script Not Found**: Double check that the path to `scripts/thinkingroot_sdk_bridge.mjs` is configured correctly in `.env`.

## API Specification

OwnPager exposes a REST endpoint for external client integrations:

### POST `/api/turn`

Exposes the agent runner to web intakes.
* **Request Body**:
  ```json
  {
    "message": "User query requirement string",
    "profile": "Session identifier profile name",
    "client_session_id": "Unique client UUID"
  }
  ```
* **Response Body**:
  ```json
  {
    "request_id": "Session request UUID",
    "final_answer": "Aggregated markdown answer string",
    "usage_estimate": {
      "total_tokens": 1250,
      "api_call_count": 3
    }
  }
  ```

## Integration & Flow Control

* **Pre-Turn Analysis**: The query text is matched against rules to identify complex workflows. If complex, the task is delegated to a ThinkingRoot Flow run.
* **Autonomous Autonomy**: `OWNPAGER_FLOW_MODE=auto` runs simple queries locally via the LLM tool loop, while complex plans run on the server.
* **Evidence Aggregation**: If the tool loop reaches its iteration limit, the system summarizes terminal stdout logs into a coherent final answer.
* **State Isolation**: Subprocesses running under target configurations run isolated from main processes to protect the host machine from leakage.

## License

OwnPager is released under the MIT License. Details can be found in the accompanying LICENSE file.

For inquiries or support, please check the [ThinkingRoot GitHub Repository](https://github.com/DevbyNaveen/ThinkingRoot).

---
