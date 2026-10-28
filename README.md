# 🦀 Agent Guidance MCP Server

[![Version](https://img.shields.io/badge/version-1.8.16-blue.svg)](https://github.com/JunMystery/Agent-Guidance/releases)
[![NPM Version](https://img.shields.io/npm/v/agent-guidance.svg?color=crimson)](https://www.npmjs.com/package/agent-guidance)
[![License: MIT](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

![Agent Guidance Hero Banner](docs/images/hero-banner.png)

> **The Architectural Conductor for AI Coding Agents.**
> Stop spaghetti code, cut context bloat by 80%, and supercharge agent reasoning. Agent Guidance supervises AI Coding Agents (Claude Code, Cursor, Windsurf, Devin, Codex) by enforcing architectural patterns, preventing monolithic slop with a 300 LOC cap, and delivering sub-millisecond **Hierarchical GraphRAG** with neural skill retrieval.

---

## 🚀 Quickstart & Installation

Installers provide an interactive mode selector:
- **`[1] Full Standalone`**: Single binary with in-process Candle/ORT inference (~40 MB).
- **`[2] Lightweight Client`**: Zero-ML footprint (~15 MB RAM), forwards queries to Remote Worker.
- **`[3] Dedicated Server Worker`**: Central ML worker daemon with systemd / launchd / Windows Scheduled Task.

### Quick Install & Run

**Universal NPM Runner (Windows, macOS, Linux — Interactive Wizard & Profiles):**
```bash
# Interactive setup wizard (Select profile: Standalone, Client, Server, or Uninstall):
npx -y agent-guidance

# Or run commands directly:
npx -y agent-guidance --setup
npx -y agent-guidance --set-server http://<SERVER_IP>:11998
npx -y agent-guidance --uninstall
```

**Windows (PowerShell In-Memory — Safe, no ExecutionPolicy bypass):**
```powershell
irm https://raw.githubusercontent.com/JunMystery/Agent-Guidance/main/scripts/install.ps1 | iex
```

**Linux / macOS (Bash & Curl):**
```bash
curl -fsSL https://raw.githubusercontent.com/JunMystery/Agent-Guidance/main/scripts/install.sh | bash
```

**macOS Homebrew:**
```bash
brew install JunMystery/tap/agent-guidance
```

### Manual Cargo Build

```bash
git clone https://github.com/JunMystery/Agent-Guidance.git
cd Agent-Guidance
cargo build --release
./target/release/agent-guidance --setup
```

---

## ⚡ The Difference: AI with vs. without Agent Guidance

| Challenge | Without Agent Guidance (The AI Slop Problem) | With Agent Guidance (The Disciplined Architect) |
| :--- | :--- | :--- |
| **Code Structure & Modularity** | AI writes sprawling 800–1,500 LOC monolithic files, mixing database queries directly into UI components. | **Enforced 300 LOC Hard Cap** & upfront pattern blueprints (**Clean Architecture**, **Layered**, **Package-by-Feature**). Files are decomposed from line 1. |
| **Container & File Naming** | Creates messy junk-drawer files (`utils.ts`, `services.py`, `helpers.go`). | **40 Compound Plural Suffixes Blocked** (`COMPOUND_FILE_NAME_PROHIBITED`). Every file must strictly observe Single Responsibility. |
| **Context Window & Tokens** | Dumps full files and 500-line markdown skills into prompt, exhausting context windows and triggering hallucinations. | **Surgical Section-Level RAG Slicing & AST Skeletonizer**: Delivers only top relevant sections (~350–800 tokens, **~80% token savings**) and collapses >300 LOC files into clean AST signatures while strictly preserving code block indentation and line numbers for diff tools. |
| **Codebase Onboarding** | AI reads files sequentially, taking dozens of turns to guess relationships and losing track of dependencies. | **10-Second Hierarchical GraphRAG**: Instant macro community outline (Level 0/1/2) and 1-hop caller/callee bundling under 250 LOC. |
| **Test & Fix Cycles** | Enters infinite repair loops, repeatedly making blind edits that break adjacent modules. | **Fix Circuit Breaker**: Trips automatically after 3 consecutive failed attempts, rolling back snapshots and requesting human intervention. |
| **Deployment Topology** | Heavy local Python/ML dependencies consume 1GB+ RAM, slowing down developer laptops. | **Dual-Role Topology**: Lightweight Client runs in **~15MB RAM**, delegating heavy neural vector workloads to a dedicated Remote ML Worker. |

---

## 🧭 Instant Codebase Onboarding with Hierarchical GraphRAG

AI agents waste up to 70% of their context reading full source files just to understand function call chains. Agent Guidance implements **Hierarchical Leiden GraphRAG** backed by Tree-sitter AST symbol tables and SQLite:

![Hierarchical GraphRAG Architecture](docs/images/graphrag-architecture.png)

### The 3-Tier Community Hierarchy
- **Level 0 (Macro Subsystems)**: Broad domain boundaries (`ui`, `domain`, `infrastructure`, `ml`).
- **Level 1 (Feature Modules)**: Logical service groups and package boundaries.
- **Level 2 (Micro Clusters)**: Tightly-coupled function and class clusters.

```text
┌─────────────────────────────────────────────────────────────────────────────┐
│ LEVEL 0: MACRO SUBSYSTEMS (Macro Architecture Layer)                        │
│   [ UI & Presentation ]        [ Domain & Logic ]        [ Infrastructure ] │
└─────────────┬───────────────────────────┬─────────────────────────┬─────────┘
              │                           │                         │
              ▼                           ▼                         ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ LEVEL 1: FEATURE MODULES (Leiden Community Clusters)                        │
│   [ Authentication Service ]   [ Billing Engine ]        [ Rusqlite Store ] │
└─────────────┬───────────────────────────────────────────────────────────────┘
              │
              ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ LEVEL 2: MICRO CALL GRAPH (AST Directed Function Nodes)                     │
│   verify_token() ──────► check_permissions() ──────► query_user()           │
└─────────────────────────────────────────────────────────────────────────────┘
```

### The 4 GraphRAG Search Modes
1. **DRIFT Search (Default)**: Dual-route search combining macro community summaries with micro AST signatures for complete structural grounding.
2. **Global Search**: High-level architectural reasoning across macro community summaries.
3. **Local Search**: Targeted symbol search with 1-hop and 2-hop DAG call/import fan-out.
4. **Subgraph Bundling (`subgraph_bundle`)**: Packs target function definition + 1-hop callers + 1-hop callees into a single token-bounded snippet (<250 LOC).

---

## 📉 Surgical Skill RAG Slicing (Token Savings)

Traditional prompt injection dumps entire 300–600 line skill files into the context window, causing prompt bloat, high costs, and attention drift. Agent Guidance splits skills by `##` headers, computes vector embeddings, and delivers only the exact 2–3 sections relevant to the active task:

```text
TRADITIONAL FULL SKILL INJECTION:
[██████████████████████████████████████████████████] ~2,100 tokens (Full SKILL.md Dump)

AGENT GUIDANCE SURGICAL RAG SLICING:
[████████] ~350 tokens (Top 2-3 Vector-Ranked Sections)
═════════════════════════════════════════════════════════════════════════════
🚀 ~83.3% CONTEXT SAVED PER SKILL CALL | < 0.2ms In-Memory Binary AGV1/AGS1
```

- **Full Guidance for Standard Skills ($\le 250$ LOC)**: Compact skills preserve complete instructions, checklists, and code patterns without destructive slicing.
- **Surgical Slicing for Reference Skills**: Large skill catalogs (> 250 LOC) are sliced to top 5 vector-ranked sections with YAML frontmatter and hollow parent headers filtered out.
- **In-Memory Binary Runtime (`AGV1` & `AGS1`)**: Zero disk reads or live markdown parsing during queries.
- **HTTP 304 Zero-Payload Caching**: Returns empty `304 Not Modified` when catalog ETag matches, cutting network round-trips to `<0.2 ms`.
- **Local SQLite LRU Cache**: Repeated skill selections resolve locally from disk in `<1 ms`.

---

## 🛡️ Step-by-Step AI Guardrail Pipeline

Agent Guidance forces autonomous agents to follow an enterprise engineering lifecycle:

![Orchestrator Workflow & Safety Gates](docs/images/orchestrator-workflow.png)

```text
  [ User Prompt ]
         │
         ▼
┌─────────────────┐
│  task_pipeline  │ ──► Priority Gate & Architecture Pattern Detection
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│   Plan Stage    │ ──► 300 LOC Upfront Decomposition Blueprint
└────────┬────────┘
         │
         ▼
┌─────────────────┐       No (Revise)
│ Human Approval? ├─────────────────────────┐
└────────┬────────┘                         │
         │ Yes                              ▼
         ▼                          ┌───────────────┐
┌─────────────────┐                 │ Ask / Revise  │
│   Build Stage   │                 └───────▲───────┘
│  workflow_gate  │ ──► 300 LOC Ceiling     │
└────────┬────────┘     Rollback Snapshot   │
         │                                  │
         ▼                                  │
┌─────────────────┐                         │
│  Test & Verify  │                         │
└────────┬────────┘                         │
         │                                  │
         ├────── Pass ────────► [ Review & Document ]
         │                                  │
         ├────── Fail (<= 3 attempts) ──────┤ (Auto Fix Loop)
         │                                  │
         └────── Fail (> 3 attempts) ───────┘ (Circuit Breaker Trips)
```

1. **Priority & Architecture Gate (`task_pipeline`)**: Mandatory first step. Detects project architecture, flags monolithic files ($\ge 200$ LOC), and generates modular blueprints.
2. **Human-in-the-Loop Plan Approval**: Code modification remains strictly locked until `plan_approved = true`.
3. **Write-Through Authorization (`workflow_gate`)**: Creates pre-edit rollback snapshots, blocks compound plural containers, and enforces the 300 LOC hard ceiling.
4. **Predictive Co-Change Sentinel**: Evaluates historical commit graphs to warn agents if coupled files or test suites were forgotten.
5. **Fix Circuit Breaker**: Automatically trips after 3 consecutive failed test attempts, halting hallucinated fix spirals.

---

## 🌐 Dual-Role Topology (v1.8.7)

Split your AI coding workloads to fit your development environment:

![Dual-Role Client/Server Topology](docs/images/dual-role-topology.png)

```text
Personal Developer Device (Client)             Remote Server (Worker Daemon)
┌───────────────────────────────────────┐      ┌───────────────────────────────────────┐
│ • ZERO Local ML & Vector Files        │      │ • SOLE CUSTODIAN of Binary Vectors:   │
│   (skills.bin & vectors.bin ABSENT)   │      │     vectors.bin & skills.bin (<1ms)   │
│ • Tree-sitter AST & Graph Engine      │      │ • Multi-Threaded Worker Pool (4x)     │
│ • Local SQLite (.agent-context/*.db)  │      │ • Candle BERT Embedder in VRAM/RAM    │
│ • File Reading & 300 LOC Diff Guards  │      │ • ONNX Cross-Encoder Reranker         │
│ • RAM Footprint: ~15 MB               │      │ • 1-N Compaction & Tombstone Engine   │
│ • Auto Task Context Forwarding        │      │ • RAM Footprint: ~600–800 MB          │
└──────────────────┬────────────────────┘      └──────────────────▲────────────────────┘
                   │                                              │
                   │ (HTTP Keep-Alive Pool / Zero-Payload 304)    │
                   ├─────────── POST /api/skills/search ──────────┤
                   ├─────────── POST /api/skills/slice ───────────┤
                   ├─────────── GET  /api/skills/stats ───────────┤
                   └─────────── GET  /health ─────────────────────┘
```

- **Lightweight Client (~15 MB RAM)**: Ideal for laptops. Runs local AST indexing and safety gates; delegates all semantic vector retrieval to the remote worker.
- **Dedicated Server Worker**: Centralizes skill storage for engineering teams, compiling staged custom skills into compacted binary matrices with atomic hot-swapping (<1ms).

---

## 💻 CLI Command Matrix

```bash
agent-guidance [OPTIONS]

Client & IDE Configuration:
  --setup                  Install and configure MCP server across all IDE clients
  --verify-setup           Verify MCP configuration paths across IDEs
  --upgrade                Download and install latest release package, update IDE configs
  --set-server <URL|local> Configure remote ML worker endpoint (or 'local' for standalone)
  --test-server            Test connection and measure ping latency to remote ML worker
  --stats, --status        Display client mode, system telemetry, and remote skill stats
  --uninstall              Remove MCP server configurations from all IDE clients

Remote Worker & Server Administration:
  --server                 Start remote ML worker daemon (default: http://127.0.0.1:11998)
  --worker-port <PORT>     Custom ML worker port (default: 11998)
  --bind <ADDR>            Network bind address (e.g. 0.0.0.0 or 127.0.0.1)
  --api-key <KEY>          Bearer authentication token for remote worker
  --setup-server           Interactive CLI setup wizard for remote ML worker & OS daemon
  --reindex-skills         Compile staging skills into in-memory binary format
  --delete-skill <NAME...> Prune 1-N skills from binary bundle and staging with tombstones

Daemon & Maintenance:
  --daemon, -d             Force start in background singleton daemon mode
  --proxy                  Force connect as client proxy to daemon; exit if no daemon
  --dashboard              Start real-time web usage dashboard at http://127.0.0.1:11997
  --port, -p <PORT>        Custom dashboard port (default: 11997, alias: --dashboard-port)
  --project <PATH>         Filter dashboard to a specific project path or name
  --prune-missing          Prune non-existent projects from usage tracking registry
  --cleanup                Auto-clean expired logs, prune dead projects, and vacuum SQLite DB
  --retention-days <N>     Retention window in days for detail logs (default: 7)
  --help, -h               Print help message
```

---

## 🛠️ MCP Tool Suite Reference

| Tool Name | Role / Action | Key Capabilities |
| :--- | :--- | :--- |
| **`task_pipeline`** | **Entrypoint Orchestrator** | CALL FIRST. Unlocks priority gate, detects architecture, injects memorized learnings, and synthesizes upfront modular blueprints (<300 LOC mandate). |
| **`select_skills`** | **Semantic Skill Loader** | Delivers full instructions for standard skills ($\le 250$ LOC) and vector-ranked section slices for large reference skills (up to 5 sections, ~80% token savings), records usage analytics, and injects language safety micro-rules. |
| **`workflow_gate`** | **Stage & Impact Guard** | Manages stage transitions (`Context` $\rightarrow$ `Plan` $\rightarrow$ `Build` $\rightarrow$ `Test` $\rightarrow$ `Fix` $\rightarrow$ `Review`), enforces 300 LOC hard ceiling, blocks compound plural containers, and controls fix circuit breakers. |
| **`project_context`** | **GraphRAG & AST Search** | Hierarchical Leiden GraphRAG (`drift`, `global`, `local`), Subgraph Bundling (<250 LOC definition + 1-hop callers/callees), intra-procedural data flow, AST skeletonization, and 6-phase instant cascade (<100ms). |
| **`guidance`** | **Standards & Skill Catalog** | 2-stage Candle BERT vector search + Cross-Encoder reranking over 279 embedded skills, slash command stripping & neutral bypass, question intent detection, mentioned skill prioritization (`(Recommended)`), pre-code architecture checklists, and empirical verification contracts. |
| **`session_continuity`** | **Memory & Snapshots** | Persists session memory, tracks pairwise co-changes into evolutionary git graphs, stores categorised project learnings, and generates cross-agent handoff briefs. |

### 🛡️ Smart Slash Command & Intent Discovery (v1.8.11)

- **Slash Command Stripping & Neutrality**: Slash command tokens (`/<cmd>` like `/plan`, `/generative_ui`) never auto-load skills; they are cleanly stripped from the prompt so search runs only on the actual task text. If a query contains only slash commands, skill proposals are bypassed entirely.
- **Informational Q&A Bypass**: Pure informational questions (ending in `?` or starting with question/explanation keywords) automatically bypass skill proposal gates.
- **Direct Skill Mention Prioritization**: When a user mentions a skill name in prose, it is prioritized at `options[0]` with `(Recommended) ` in the `ask_question` confirmation dialog.
- **Generative UI Protocol Enforcement**: Mandates visual HTML artifact rendering (`UserFacing: true`) and inline embedding (`<agent-embed>`) before soliciting layout choices, eliminating "blind choice" prompts (`BLIND_UI_CHOICE_PROHIBITED`).

### 🔍 Multi-Entity Cascade Code Search & Vector DB Upgrade (v1.8.13)

- **Layer 1 File Path Matcher (`[file match]`)**: Sub-millisecond lookup in `files` table supporting exact path, basename (`%/target.rs`), and substring matches, enriched with real-time AST outline of top 3–5 symbols and boosted with `score: 2.50`.
- **Layer 2 Smart Token & Qualifier Expansion**: Automatic decomposition of qualified paths (`Parent::member`, `object.method`), parallel case swapping (`camelCase` ↔ `snake_case`), conversational stop-word removal, and qualifier BM25 boosting (1.35x).
- **Layer 3 Lexical Content Fallback**: BM25 FTS5 retrieval over code chunks when symbol results are sparse.
- **Layer 4 Vector DB & Hybrid Fusion**: Rich code chunk previews (`chunk_text`), extended symbol signatures (`parent`, `receiver`, `end_line`), and automatic neural vector retrieval when FTS results are insufficient or score < 0.85.
- **Accented Multilingual Confirmation**: Guaranteed full diacritics and native language translation for skill proposal dialogs (`ask_question`).

### 🛑 MCP Daemon Auto-Shutdown & Hard Idle Cap (v1.8.14)

- **Hard Max Idle Cap (`DEFAULT_MAX_IDLE_COOLDOWN_SECS = 180s`)**: Automatically terminates the daemon if 0 clients remain connected for 3 minutes, even when lingering IDE background processes or system tray services are detected.
- **Deep Process Filtering & Command-Line Inspection**: Inspects process arguments via `ProcessRefreshKind::new().with_cmd(...)`, filtering out `--type=crashpad-handler`, `--type=utility`, `code-tunnel`, and recognizing CLI tools (`agy`, `gemini`, `codex`).
- **Synchronous SQLite Shutdown Logging & Clean Exit**: Flushes lifecycle events synchronously to `usage.db` via `log_mcp_direct` before terminating background threads cleanly via `std::process::exit(0)`.

### 🎨 Generative UI Strict ask_question Prohibition (v1.8.15)

- **Zero-Block Generative UI Flow**: Completely prohibits `ask_question` tool invocations whenever `/generative_ui` is active or detected.
- **Skill Proposal & Modal Bypass**: Automatically bypasses skill proposal dialogs on `/generative_ui` tasks, eliminating blocking confirmation modals in Antigravity and Gemini.
- **Immediate Inline Preview Mandate**: Directs agents to immediately create self-contained HTML artifacts (`UserFacing: true`) and embed them inline via `<agent-embed>` without soliciting premature layout choices.

### 🎯 Two-Stage Tiered Skill Engine & Context Bleed Isolation (v1.8.16)

- **Two-Stage Skill Query Discovery**:
  - *Stage 1 (Clean Prompt Search)*: Directly evaluates the user prompt without historically polluted `recent_tasks` or modified files string concatenation, eliminating false-positive matches (such as `video-editing` leaking into backend tasks).
  - *Stage 2 (Tech Stack Expansion)*: Intelligently expands query with repository profile technologies and languages only if Stage 1 yields partial relevance, preserving zero false-positives on conversational prompts.
- **Tiered Multi-Intent Ranking**:
  - Automatically classifies candidate skills into Core Domain/Feature skills (prioritized at `options[0]` with `(Recommended)`) and Tooling skills (Git, CI/CD, Docs) as secondary options.
- **Negative Domain Filtering**:
  - Strips `"media"` from universal keywords and enforces negative domain blocking to prevent video/audio/3D/game skills from intruding on Rust/backend/CLI repositories.
- **Workspace Boundary Guard & Cross-Workspace Warning Gate**:
  - Scopes active workspace roots over global memory to prevent cross-workspace context bleed.
  - Warns and blocks accidental external file modifications unless explicit `allow_cross_workspace=true` is declared.
- **Multi-IDE & Agent Native ID Tracking**:
  - Automatically detects and binds native conversation and session UUIDs (`conversation_id`, `session_id`, `chat_id`) across Antigravity, Gemini, Claude Code, Cursor, and VS Code.

---

## ⚡ Tool Micro-Rules & Dashboard Editor

Every core MCP tool is coupled with targeted execution micro-rules that keep LLMs aligned with project conventions without cluttering global prompts:

- **Dual-Scope Hierarchy**:
  1. **Per-Project Override** (`<project>/.agent-context/tool_rules.toml`): Repo-specific guardrails.
  2. **Global Configuration** (`~/.agent-guidance/config.toml`): System-wide default conventions.
  3. **Built-in Defaults**: High-signal, battle-tested engineering mandates.
- **Web Dashboard Editor (`http://127.0.0.1:11997/#rules`)**: Visual management tab featuring:
  - Per-tool Enable/Disable toggle switches.
  - Markdown rule editors with multi-line auto-expansion.
  - One-click **Reset Default** and project-level **Revert to Global**.
  - Dynamic synchronization with the tracked project dropdown.
- **Runtime Injection**: The effective rule is automatically attached to the response payload as a high-visibility `> [!IMPORTANT] **Tool Execution Mandate**` banner upon tool execution.

---

## ⏱️ Global Timeframe Analytics & Adaptive Telemetry

The Web Dashboard (`http://127.0.0.1:11997`) features an interactive persistent Command Deck with global timeframe controls:

- **Persistent Command Bar**: Sticky top bar housing the **Active Scope** project combobox and **Timeframe Tabs** (`24H` / `7D` / `30D` / `ALL`), synchronizing telemetry views seamlessly across both Dashboard and Actions Log.
- **Server-Side SQLite Filtering**: Switching timeframes dynamically recalculates token metrics, tool breakdown distributions, top active skills, recent action streams, and governance cadence.
- **Adaptive Invocations & Latency Wave**:
  - `24H`: 24 hourly buckets (`HH:00`) for fine-grained intraday execution rhythms.
  - `7D` & `30D`: Daily milestone buckets (`DD/MM`) displaying longitudinal token optimization trends.
  - `ALL`: Dynamic multi-day historical bucketing spanning full SQLite telemetry records.
- **Two-Tier Compact Rollup & Scoped Clean**:
  - Raw 7-day detail logs automatically compact into lightweight daily rollups (`dashboard_daily_rollups`), cutting disk usage by >98% while ensuring 30D/ALL analytics persist indefinitely.
  - One-click **Clean** button in HUD command bar to purge historical dashboard metrics per active project or globally without affecting diagnostic logs.
- **Clean Minimal Sidebar**: Polished sidebar navigation with clean text indicators for distraction-free monitoring.

---

## 🎨 In-Browser Skill Editor & Staging Override

The Web Dashboard features a centralized Skill Registry and Markdown editor (`http://127.0.0.1:11997/#skills`):

- **In-Browser Authoring & Live Preview**: Split-screen Markdown editor with real-time rendered preview, syntax styling, and tab-indentation support.
- **Global Staging Precedence (`~/.agent-guidance/staging/skills/`)**: Edits safely override embedded built-in skills without altering git repository assets or source files.
- **Instant Precedence & Fallback**: `load_all_skills` immediately picks up staged skills. One-click **Revert to Default** purges staging overrides to restore built-in embedded originals.
- **Custom Skill Creation**: Add brand-new domain skills via the `➕ New Skill` button.
- **Binary Re-Indexing**: Integrate with **⚡ Compile to Binary** to refresh the high-speed AVX-512 vector index (`skills.bin` & `vectors.bin`) on demand.

---

## 📄 License

Distributed under the MIT License. See [LICENSE](LICENSE) for details.
