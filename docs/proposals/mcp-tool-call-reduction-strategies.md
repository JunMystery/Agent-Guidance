# Architecture Proposal: MCP Tool Call Reduction Strategies

**Document Version**: 2.0.0  
**Status**: Implemented & Validated (Released in v2.0.0)  
**Target Systems**: `Agent Guidance Rust MCP Server`, `AGENTS.md`, `GEMINI.md`, `CLAUDE.md`  

---

## 1. Executive Summary & Problem Statement

Autonomous AI coding agents frequently emit redundant or sequential MCP tool calls during development cycles. In complex workflows, an agent may make 8–15 tool calls before writing a single line of code:
1. `task_pipeline` (discovery)
2. `guidance(search)` (redundant search)
3. `ask_question` / `select_skills` (skill injection)
4. `project_context(search)` (file search 1)
5. `project_context(search)` (file search 2)
6. `project_context(read)` (file A)
7. `project_context(read)` (file B)
8. `project_context(callers)` (dependency inspection)
9. `workflow_gate(authorize_edit)` (file A)
10. `workflow_gate(authorize_edit)` (file B)

This pattern introduces significant overhead:
- **High Latency**: Each roundtrip to the LLM and MCP server costs 2–6 seconds, accumulating to 30–60 seconds of idle wait time.
- **Context Bloat**: Sequential tool call and tool result message pairs fill the context window with repetitive scaffolding.
- **Provider API Costs**: Unnecessary tool calls consume tokens on both input prompting and JSON output parsing.

This document outlines concrete strategies across **Protocol & Prompt Engineering**, **Batch Tool Primitives**, and **Server-Side Architectural Enhancements** to reduce tool call frequency by **60% to 80%**.

---

## 2. Root Cause Analysis

| Inefficient Pattern | Underlying Cause | Consequence |
|---|---|---|
| **Sequential File Reads** | Agent inspects files one-by-one (`read` file A, wait, `read` file B, wait). | 3–5 roundtrips for multi-file context gathering. |
| **Sequential Edit Authorizations** | Agent calls `authorize_edit` individually for each file discovered during work. | 1 extra roundtrip per edited file. |
| **Exploration Scaffolding** | Agent uses separate calls for symbol search, definition lookup, caller lookup, and callee lookup. | 3–4 roundtrips to understand a single function. |
| **Protocol Rigidity on Minor Tasks** | The execution lifecycle forces full ceremony (task pipeline $\to$ plan $\to$ gate $\to$ verification) even for 1-line doc fixes or typo patches. | 5+ tool turns for trivial modifications. |
| **Turn 1 Redundancy** | Agent ran separate `task_pipeline` and `guidance(search)` steps. *(Partially solved in v1.9.0 via auto-bundling).* | 1–2 wasted turns at initialization. |

---

## 3. Strategy 1: Prompt & Protocol Engineering (Immediate Zero-Code Wins)

These optimizations can be enacted immediately by tuning the protocol rules in `AGENTS.md`, `GEMINI.md`, and `CLAUDE.md`:

### 3.1 Clustered Read Mandate
Enforce that related files must always be read in a single turn using `relative_paths`:
```json
{
  "operation": "read",
  "relative_paths": [
    "src/mcp/tools/context.rs",
    "src/mcp/tools/helpers_path.rs",
    "src/mcp/tools/helpers.rs"
  ]
}
```
*Impact*: Replaces $N$ roundtrips with exactly **1 turn**.

### 3.2 Pre-Authorization During Plan Approval
Instead of approving the plan and then issuing a subsequent `workflow_gate(action="authorize_edit")` turn, combine them:
```json
{
  "action": "approve_plan",
  "user_confirmed": true,
  "project_path": "/path/to/project",
  "relative_paths": [
    "src/mcp/tools/context.rs",
    "src/mcp/tools/helpers_path.rs"
  ],
  "justification": "Implement planned architectural fix"
}
```
*Impact*: Eliminates the separate authorization turn completely (**0 additional turns**).

### 3.3 Fast-Path Exemption for Minor & Non-Code Tasks
Add an explicit protocol exemption clause in `AGENTS.md`:
> **Minor Task / Doc Exemption**:
> For pure documentation updates (`.md`), comments, typos, or single-line configuration tweaks (< 10 LOC):
> - Bypass formal `implementation_plan.md` generation.
> - Bypass skill proposal prompts (`ask_question` / `select_skills`).
> - Directly authorize and edit via `workflow_gate(action="authorize_edit", relative_paths=[...])`.

### 3.4 Multi-Tool Parallel Turn Emission
Encourage the LLM client to emit independent tool calls in parallel within the same turn (supported by Antigravity, Claude, and Gemini API):
```json
[
  { "name": "project_context", "args": { "operation": "read", "relative_path": "Cargo.toml" } },
  { "name": "project_context", "args": { "operation": "symbols", "relative_path": "src/main.rs" } }
]
```

---

## 4. Strategy 2: Utilizing High-Density Primitives

The `agent-guidance` MCP server already contains dense primitives that consolidate multiple queries into single outputs:

### 4.1 Subgraph Bundling (`subgraph_bundle`)
Instead of issuing:
1. `project_context(operation="search", query="handle_read")`
2. `project_context(operation="definition", ...)`
3. `project_context(operation="callers", ...)`
4. `project_context(operation="callees", ...)`

Invoke **Subgraph Bundling** directly:
```json
{
  "operation": "subgraph_bundle",
  "target_symbol": "handle_read"
}
```
*Result*: Delivers the target function definition + 1-hop callers + 1-hop callees compressed into a bounded single context snippet (< 250 LOC) in **1 turn**.

### 4.2 Hierarchical GraphRAG DRIFT Search
Instead of multiple keyword searches across files, use DRIFT search:
```json
{
  "operation": "graph_rag",
  "query": "workspace root detection and path traversal validation"
}
```
*Result*: Synthesizes macro Leiden community clusters (Level 0/1) with micro AST signatures in a single grounded response.

---

## 5. Strategy 3: Server-Side Roadmap Enhancements

To make the MCP server proactively reduce calls without relying entirely on prompt adherence, the following server-side capabilities can be implemented:

### 5.1 Auto-Authorization on Plan Approval
- **Behavior**: When `workflow_gate(action="approve_plan")` is called without explicit `relative_paths`, the server automatically parses the target file paths from the active session's `implementation_plan.md` artifact on disk and pre-authorizes them automatically.
- **Turn Reduction**: 1 turn saved per task.

### 5.2 Unified Read-and-Authorize Primitive (`authorize: true`)
- **Behavior**: Add an optional `authorize: bool` flag to `project_context(operation="read")`:
  ```json
  {
    "operation": "read",
    "relative_paths": ["src/mcp/tools/context.rs"],
    "authorize": true,
    "justification": "Refactoring workspace detection"
  }
  ```
- **Turn Reduction**: Eliminates the need for a separate `workflow_gate` call when inspecting code intended for immediate editing.

### 5.3 Auto-Advancing Workflow Transitions
- **Behavior**: If `workflow_gate(action="set_stage", target_stage="Build")` is called, automatically set `plan_approved = true` when user approval was recorded in context or passed via `user_confirmed: true`, preventing a separate roundtrip.

### 5.4 Session-Bounded AST Response Cache
- **Behavior**: If an agent requests `project_context(read)` on a file that has already been read in the current turn or last 30 seconds (and whose file mtime has not changed), return cached bounded output without re-invoking tree-sitter or filesystem stat operations.

---

## 6. Implementation & Impact Matrix

| Optimization | Category | Complexity | Expected Turn Reduction | Implementation Status | Test Coverage |
|---|---|---|---|:---:|---|
| **Clustered Reads (`relative_paths: [...]`)** | Prompt / Tool Primitive | Low | **2–4 turns per task** | **Completed (v2.0.0)** | `src/mcp/tools_cluster_tests.rs` (6/6 tests) |
| **Pre-Authorization in `approve_plan`** | Protocol / Usage | Low | **1 turn per task** | **Completed (v2.0.0)** | `src/mcp/tools_plan_auth_tests.rs` (5/5 tests) |
| **Minor Task Fast-Path Exemption** | Protocol / Engine | Low | **3–5 turns on small tasks** | **Completed (v2.0.0)** | `src/mcp/tools_fast_path_tests.rs` (5/5 tests) |
| **Subgraph Bundling Upgrades** | Tool Primitive | Medium | **2–3 turns per investigation** | **Completed (v2.0.0)** | `src/mcp/tools_bundle_upgrade_tests.rs` (4/4 tests) |
| **Server-Side Auto-Auth from Plan Artifact** | Server Code | Medium | **1 turn per task** | **Completed (v2.0.0)** | `src/mcp/tools_plan_auth_tests.rs` (5/5 tests) |
| **Unified `read_and_authorize` Primitive** | Server Code | Medium | **1 turn per task** | **Completed (v2.0.0)** | `src/mcp/tools_read_auth_tests.rs` (5/5 tests) |
| **Auto-Advancing Transitions** | Server Code | Low | **1 turn per task** | **Completed (v2.0.0)** | `src/mcp/tools/pipeline.rs`, `helpers_auth.rs` |
| **Session-Bounded AST Response Cache** | Server Code | Medium | **1–3 turns per review** | **Completed (v2.0.0)** | `src/mcp/tools_cache_tests.rs` (5/5 tests) |

### Cumulative Impact
- Typical Task Turn Count before optimizations: **10–14 tool turns**
- Typical Task Turn Count with v2.0.0 reduction engine: **3–5 tool turns**
- **Net Reduction: ~65% fewer tool invocations, ~70% lower turn-wait latency.**

---

## 7. Implementation Verification & Module Directory

| Strategy | Primary Module | Helper / Registry | Verification Suite |
|---|---|---|---|
| **3.1 Clustered Read & Target Path Spec** | `src/mcp/tools/context_read_cluster.rs` | `helpers_spec.rs` | `tools_cluster_tests.rs` |
| **3.2 Pre-Authorization in approve_plan** | `src/mcp/tools/gate_approval.rs` | `state/types.rs` | `tools_plan_auth_tests.rs` |
| **3.3 Minor Task Fast-Path Exemption** | `src/mcp/tools/pipeline_fast_path.rs` | `pipeline.rs`, `gate_edit.rs` | `tools_fast_path_tests.rs` |
| **3.4 Multi-Tool Parallel Turn Emission** | `AGENTS.md`, `GEMINI.md`, `CLAUDE.md` | Protocol rules | Prompt compliance |
| **4.1 Subgraph Bundling Upgrades** | `src/mcp/tools/context_bundle.rs` | `helpers_spec.rs` | `tools_bundle_upgrade_tests.rs` |
| **4.2 Hierarchical GraphRAG DRIFT Search** | `src/context/graph_rag/` | `context.rs` | Regression test suite |
| **5.1 Server-Side Auto-Auth from Plan** | `src/mcp/tools/gate_plan_parser.rs` | `gate_approval.rs` | `tools_plan_auth_tests.rs` |
| **5.2 Unified read_and_authorize** | `src/mcp/tools/helpers_auth.rs` | `context_read.rs`, `context.rs` | `tools_read_auth_tests.rs` |
| **5.3 Auto-Advancing Transitions** | `src/mcp/tools/pipeline.rs` | `helpers_auth.rs` | `tools_fast_path_tests.rs` |
| **5.4 Session-Bounded AST Cache** | `src/mcp/tools/context_cache.rs` | `context_read.rs`, `types.rs` | `tools_cache_tests.rs` |
