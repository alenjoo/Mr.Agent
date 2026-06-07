# OwnPager

OwnPager is an intelligent agent and requirement console application designed to execute complex tasks and manage autonomous agent workflows. It features a high-performance backend written in Rust and a minimalist web frontend. 

The system tightly integrates with [ThinkingRoot DB](https://github.com/DevbyNaveen/ThinkingRoot) for state management, flow control, and branch orchestration.

## Architecture overview

OwnPager employs a highly decoupled, state-of-the-art architecture split across a Rust backend daemon, a TypeScript frontend, and a cloud-based state management layer. This robust design guarantees that the agent can execute complex, multi-step, long-running workflows securely without blocking the user interface or losing execution state during system restarts.

### 1. High-Level Component Interactions

The architecture follows an event-driven and RESTful hybrid model:
1. **User Input Phase**: The user submits a requirement via the TypeScript frontend.
2. **Intake & Routing**: The Rust backend receives the requirement via an Axum-based API endpoint and creates an initial execution state in ThinkingRoot DB.
3. **Reasoning Loop**: The backend enters a continuous reasoning loop, querying the OpenAI API for the next optimal action.
4. **Tool Execution**: If a tool is selected by the LLM (e.g., executing a bash command or writing a file), the backend validates the action against security policies and executes it locally.
5. **State Synchronization**: Throughout the process, the agent's memory, context window, and tool outputs are continuously synced with ThinkingRoot DB.
6. **Live Telemetry**: The frontend receives real-time streams of the agent's thought process and task status.

### 2. Backend Daemon (Rust / Axum)

The backend is engineered for maximum performance, memory safety, and concurrency. It acts as the core execution engine of the agent ecosystem.

#### 2.1 API & Routing Layer
- **High-Concurrency API Routing**: Built on `axum` and the `tokio` asynchronous runtime. This allows the backend to efficiently handle hundreds of simultaneous request streams, status update websockets, and background agent executions with minimal overhead.
- **Middleware & Tracing**: Implements comprehensive middleware for request logging, CORS handling, and authentication validation. It uses `tracing` for structured logging, allowing developers to monitor internal agent state transitions deeply.

#### 2.2 LLM Orchestration & Prompting
- **Inference Engine**: Tightly integrates with the OpenAI API for sophisticated reasoning, multi-step task decomposition, and JSON-based tool-use generation.
- **Context Management**: Implements dynamic context window management. As the agent accumulates data, the backend summarizes older context or prunes irrelevant information to ensure the LLM stays within token limits while retaining crucial task knowledge.
- **Fallback Strategies**: Includes exponential backoff and retry mechanisms for API rate limits or transient network failures during LLM inference.

#### 2.3 Local Tool Execution Engine
- **Sandboxed Execution Environment**: A secured, isolated execution environment that runs local terminal actions, shell scripts, and file manipulations.
- **Safety Configurations**: Tool execution is strictly governed by local safety configurations. Commands can be whitelisted, blacklisted, or flagged to require explicit human approval via the frontend before execution.
- **Asynchronous Output Streaming**: Long-running tool commands (like compiling a large project) have their `stdout` and `stderr` captured asynchronously and streamed back to the frontend in real-time.

#### 2.4 State & Memory Management
- **ThinkingRoot DB Interface**: Interacts continuously with the cloud-based ThinkingRoot DB to persist the agent's short-term memory, manage tool execution limits, and handle complex state transitions.
- **Resilience**: Because state is managed externally, the local Rust daemon can be safely restarted without losing the context of an ongoing agent task. Upon restart, the daemon pulls the active state from ThinkingRoot and resumes execution seamlessly.

### 3. Frontend Interface (TypeScript)

The user-facing component of OwnPager is designed for maximum clarity, utility, and speed. It avoids the heavy bundles of traditional web apps in favor of a lean, focused interface.

#### 3.1 Design Philosophy
- **High-Contrast, Minimalist UI**: Utilizes a clean, stark design language tailored specifically for text legibility and rapid requirement intake. The interface is deliberately free of unnecessary UI bloat, animations, or heavy graphical assets.
- **Terminal-Inspired Aesthetics**: The visual language draws inspiration from terminal environments, reinforcing its nature as a developer and power-user tool.

#### 3.2 Real-time Communication
- **Live Status Reporting**: Communicates with the Rust backend (via Server-Sent Events or WebSockets) to stream real-time logs, intermediate agent thoughts, and task progression. This gives the user deep visibility into "what the agent is doing right now."
- **Interactive Prompts**: When the backend encounters a blocked action requiring human approval, the frontend dynamically renders an interactive prompt for the user to approve, deny, or modify the action.

#### 3.3 Deployment Versatility
- **Embedded Context Ready**: Engineered to operate seamlessly within a standard web browser, or to be embedded within other environments (such as an Electron app, a VSCode extension, or an iframe within another dashboard).

### 4. State Management (ThinkingRoot DB)

[ThinkingRoot DB](https://github.com/DevbyNaveen/ThinkingRoot) serves as the central nervous system that coordinates and tracks the multi-agent workflows. It elevates the application from a simple script to a robust, distributed agent architecture.

#### 4.1 Flow Control & Execution Modes
- **Autonomous vs. Supervised Execution**: Manages the overarching sequence of tasks. By configuring `OWNPAGER_FLOW_MODE`, users can dictate whether the agent acts fully autonomously or requires user approval at critical junctures.
- **Conversational State**: Maintains the history of user-agent interactions, ensuring the agent remembers prior constraints and user preferences across different sessions.

#### 4.2 Branch Orchestration
- **Parallel Reasoning**: Enables the agent to fork its execution paths. If a task can be parallelized (e.g., researching three different APIs simultaneously), ThinkingRoot orchestrates the branches and aggregates the results back into the main execution thread.
- **State Rollbacks**: Provides the ability to snapshot state. If an agent goes down a "hallucination rabbit hole," the execution can be rolled back to a known good state in the ThinkingRoot tree.

### 5. Security & Isolation

Security is a primary concern given the agent's ability to execute local commands and read files.
- **Workspace Confinement**: The agent's file system access is strictly jailed to the designated `THINKINGROOT_WORKSPACE` directory. It cannot traverse up the directory tree to access sensitive OS files.
- **Environment Variable Masking**: The execution engine actively scrubs sensitive environment variables from logs and agent context to prevent accidental leakage to the LLM provider.

## Dependencies

- **ThinkingRoot DB**: The central nervous system for managing conversational state, flow structures, and agent configurations. Ensure you have your project key generated from the console.
- **OpenAI API**: Provides the core language model inference capabilities.
- **Rust Toolchain**: Required to compile the `ownpager` backend daemon.
- **Node.js**: Required to serve the static frontend assets.

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
