# 🦀 Agent Guidance MCP Server

[![Releases](https://img.shields.io/github/v/release/JunMystery/Agent-Guidance?style=flat-square)](https://github.com/JunMystery/Agent-Guidance/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg?style=flat-square)](LICENSE)

![Agent Guidance Hero Banner](docs/images/hero-banner.png)

> **The Architectural Conductor for AI Coding Agents.**
> Stop spaghetti code, cut context bloat by 80%, and supercharge agent reasoning. Agent Guidance supervises AI Coding Agents (Claude Code, Cursor, Windsurf, Gemini, Codex) by enforcing architectural patterns, preventing monolithic slop with a 300 LOC cap, and delivering sub-millisecond **Hierarchical GraphRAG** with neural skill retrieval.

---

## 🚀 Quickstart & Installation

Installers automatically detect and configure all supported IDEs (Claude Code, Claude Desktop, Cursor, Gemini, Antigravity, Windsurf).

### Universal NPX / GitHub Runner (Windows, macOS, Linux)
```bash
# Run directly via GitHub repo (always fetches latest main):
npx -y github:JunMystery/Agent-Guidance

# Or configure directly:
npx -y github:JunMystery/Agent-Guidance --setup

# Or install globally:
npm install -g github:JunMystery/Agent-Guidance
```

### Platform Installers

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

| Challenge | Without Agent Guidance (AI Slop) | With Agent Guidance (Disciplined Architect) |
| :--- | :--- | :--- |
| **Code Structure & Modularity** | AI writes sprawling 800–1,500 LOC monolithic files, mixing database queries directly into UI components. | **Enforced 300 LOC Hard Cap** & upfront blueprints (**Clean Architecture**, **Layered**, **Package-by-Feature**). Files decomposed from line 1. |
| **Container & File Naming** | Creates messy junk-drawer files (`utils.ts`, `services.py`, `helpers.go`). | **40 Compound Plural Suffixes Blocked** (`COMPOUND_FILE_NAME_PROHIBITED`). Every file strictly observes Single Responsibility. |
| **Context Window & Tokens** | Dumps full files and 500-line markdown skills into prompt, exhausting context windows. | **Surgical Section-Level RAG Slicing & AST Skeletonizer**: Delivers only top relevant sections (~350–800 tokens, **~80% token savings**) and collapses >300 LOC files into clean AST signatures. |
| **Codebase Onboarding** | AI reads files sequentially, taking dozens of turns to guess relationships and dependencies. | **10-Second Hierarchical GraphRAG**: Instant macro community outline (Level 0/1/2) and 1-hop caller/callee bundling under 250 LOC. |
| **Test & Fix Cycles** | Enters infinite repair loops, repeatedly making blind edits that break adjacent modules. | **Fix Circuit Breaker**: Trips automatically after 3 consecutive failed attempts, rolling back snapshots and requesting human intervention. |
| **Deployment Topology** | Heavy local Python/ML dependencies consume 1GB+ RAM, slowing down developer laptops. | **Dual-Role Topology**: Lightweight Client runs in **~15MB RAM**, delegating heavy neural vector workloads to a dedicated Remote ML Worker. |

---

## 🧭 Instant Codebase Onboarding with Hierarchical GraphRAG

Traditional agents waste up to 70% of context reading full source files just to understand function call chains. Agent Guidance implements **Hierarchical Leiden GraphRAG** backed by Tree-sitter AST symbol tables and SQLite:

![Hierarchical GraphRAG Architecture](docs/images/graphrag-architecture.png)

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

- **DRIFT Search (Default)**: Combines macro community summaries with micro AST signatures for complete structural grounding.
- **Global Search**: High-level architectural reasoning across macro community summaries.
- **Local Search**: Targeted symbol search with 1-hop and 2-hop DAG call/import fan-out.
- **Subgraph Bundling (`subgraph_bundle`)**: Packs target function definition + 1-hop callers + 1-hop callees into a single token-bounded snippet (<250 LOC).

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

1. **Priority & Architecture Gate (`task_pipeline`)**: Detects project architecture, flags monolithic files ($\ge 200$ LOC), and generates modular blueprints.
2. **Human-in-the-Loop Plan Approval**: Code modification remains strictly locked until user approves the plan.
3. **Write-Through Authorization (`workflow_gate`)**: Creates pre-edit rollback snapshots, blocks compound plural containers, blocks single-file edits on $\ge 300$ LOC files (requiring paired batch decomposition with new sub-modules), and enforces the 300 LOC ceiling.
4. **Post-Edit Verification Trap & Fix Circuit Breaker**: Blocks `guidance(operation="verify")` if any modified code file remains $\ge 300$ LOC, and trips the circuit breaker after 3 consecutive failed test attempts.

---

## 🛠️ MCP Tool Suite Reference

| Tool Name | Role | Primary Developer Capabilities |
| :--- | :--- | :--- |
| **`task_pipeline`** | **Entrypoint Orchestrator** | CALL FIRST. Unlocks priority gate, detects architecture, auto-bundles Turn 1 skill discovery, injects memorized learnings, and synthesizes upfront modular blueprints (<300 LOC mandate). |
| **`project_context`** | **GraphRAG & Bounded Reader** | Hierarchical Leiden GraphRAG (`drift`, `global`, `local`), Subgraph Bundling (<250 LOC definition + 1-hop callers/callees), AST skeletonization, and 6-phase instant search cascade (<100ms). |
| **`workflow_gate`** | **Stage & Edit Guard** | Enforces 300 LOC hard ceiling, blocks compound plural containers (`*utils`, `*services`), creates rollback snapshots, and manages fix circuit breakers. |
| **`guidance`** | **Standards & Skill Catalog** | 2-stage Candle BERT vector search + Cross-Encoder reranking over embedded domain skills, pre-code architecture checklists, and empirical verification contracts. |
| **`select_skills`** | **Semantic Skill Loader** | Delivers full instructions for standard skills ($\le 250$ LOC) and vector-ranked section slices for large reference skills (~80% token savings). |
| **`session_continuity`** | **Memory & Handoff** | Persists session memory, tracks pairwise co-changes into evolutionary git graphs, stores project learnings, and generates cross-agent handoff briefs. |

> Detailed version changelog and migration notes are maintained in [CHANGELOG.md](CHANGELOG.md).

---

## 💻 Essential CLI Reference

```bash
agent-guidance [OPTIONS]

Client & IDE Configuration:
  --setup                  Install and configure MCP server across all detected IDE clients
  --verify-setup           Verify MCP configuration paths and binary permissions across IDEs
  --status, --stats        Display client mode, system telemetry, and remote skill stats
  --set-server <URL|local> Configure remote ML worker endpoint (or 'local' for standalone)
  --test-server            Test connection and measure ping latency to remote ML worker
  --uninstall              Remove MCP server configurations from all IDE clients

Daemon & Maintenance:
  --dashboard              Launch real-time web usage dashboard at http://127.0.0.1:11997
  --cleanup                Auto-clean expired logs, prune dead projects, and vacuum SQLite DB
  --reindex-skills         Compile staging skills into high-speed binary vector format
  --help, -h               Print full help message (includes remote worker and server flags)
```

---

## 📊 Web Dashboard & Telemetry

Agent Guidance includes a local real-time Command Deck at **`http://127.0.0.1:11997`**:

- **Telemetry & Token Metrics**: Persistent timeframe controls (`24H` / `7D` / `30D` / `ALL`) tracking agent token savings, tool distributions, and execution latency waves.
- **Tool Micro-Rules Editor (`/#rules`)**: Visual toggles and markdown editors to configure per-project or global tool guidance (subordinate to user's main rules in `AGENTS.md` / `RULES.md`) without prompt bloat.
- **In-Browser Skill Registry (`/#skills`)**: Author, test, and live-preview custom domain skills with instant staging precedence (`~/.agent-guidance/staging/skills/`).

---

## 📄 License

Distributed under the MIT License. See [LICENSE](LICENSE) for details.
