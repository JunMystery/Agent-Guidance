# Agent Guidance MCP Usage Guide

[Back to README](../README.md)

Common usage patterns and workflows for AI agents using the Agent Guidance MCP server.

---

## Session Lifecycle: The 6-Step Pattern

Every coding task follows a strict 6-step execution lifecycle:

1. **Initialize Context & Discover Skills**: Call `task_pipeline(task="...", project_path="...", phase="plan")` to initialize project boundaries, detect architecture, auto-bundle domain skill proposals, and unlock the priority gate.
2. **Inject Skills (Optional)**: Confirm proposed skills via `ask_question` and load on-demand guides with `select_skills(skills=[...], user_confirmed=true)`.
3. **Inspect Code Graph**: Search and read symbols using token-bounded operations via `project_context(operation="search"|"read"|"symbols"|"callers"|"blast_radius", ...)` (300-line token read cap).
4. **Plan & Get User Approval**: Create/update `implementation_plan.md` and obtain explicit user consensus.
5. **Authorize Edit**: Before creating or modifying any file, call `workflow_gate(action="authorize_edit", project_path="...", relative_path="...", risk_level="LOW", justification="...")`.
6. **Empirical Verification**: Run tests and register proof via `guidance(operation="verify", verification_command="...", expected_output_keyword="...")`.

---

## Example 1: Turn 1 Initialization (`task_pipeline`)

```json
{
  "task": "Add JWT authentication to Express API with unit tests",
  "project_path": "/absolute/path/to/project",
  "phase": "plan"
}
```

Returns project file counts, detected architecture pattern (e.g. `Clean_Architecture`), dynamic split blueprint, memorized project learnings, and unlocks subsequent gated tools.

### ⚡ Minor Task Fast-Path Exemption

For trivial changes (typos, comments, docs, or single-file bugfixes < 20 lines), use Fast-Path:
```json
{
  "task": "Fix typo in README installation commands",
  "project_path": "/absolute/path/to/project",
  "is_minor": true
}
```
- **Direct Build Transition**: Automatically advances workflow stage to `Build` (`plan_approved = true`).
- **Ceremonies Bypassed**: Skips skill proposal prompts (`ask_question`) and formal `implementation_plan.md` generation.
- **Strict Boundary Guard**: Capped at max 1 code file or 3 doc files to prevent unapproved scope creep.

---

## Example 2: Project Code Context & GraphRAG (`project_context`)

### Directory Tree Scan (Default max_depth = 3)

```json
{
  "operation": "tree",
  "project_path": "/absolute/path/to/project",
  "max_depth": 3
}
```

### 6-Phase Search Cascade

```json
{
  "operation": "search",
  "project_path": "/absolute/path/to/project",
  "query": "jwt auth middleware"
}
```

### Token-Bounded File Read (300-Line Read Cap)

```json
{
  "operation": "read",
  "project_path": "/absolute/path/to/project",
  "relative_path": "src/auth/token_service.rs",
  "start_line": 1,
  "end_line": 80
}
```

### Precise Symbol Extraction

```json
{
  "operation": "read",
  "project_path": "/absolute/path/to/project",
  "relative_path": "src/auth/token_service.rs",
  "target_symbol": "verify_token"
}
```

### Unified Read-and-Authorize (Single File & Cluster)

Read code context and simultaneously authorize file modifications in **1 turn**, avoiding a subsequent `authorize_edit` call. Supports `operation="read_and_authorize"`, `operation="read_auth"`, or `authorize: true`:

```json
{
  "operation": "read_and_authorize",
  "project_path": "/absolute/path/to/project",
  "relative_path": "src/auth/token_service.rs#verify_token",
  "justification": "Refactor token verification logic"
}
```
- **Direct vs. Deferred**: In `Build` stage, grants instant edit authorization and generates pre-edit snapshots. In `Plan` stage, queues targets into `deferred_authorized_paths` for 1-turn activation upon plan approval.
- **Pending New Files**: Non-existent files are safely declared and checked for soft domain cohesion advisories (`DOMAIN_COHESION_ADVISORY`) before creation.

### Clustered Read & Universal Target Path Spec

Inspect multiple related files, exact AST symbols, and line slices in a single turn under a 1,000-line budget:

```json
{
  "operation": "read",
  "project_path": "/absolute/path/to/project",
  "relative_paths": [
    "src/auth/token_service.rs#verify_token",
    "src/config/jwt.rs:1-40",
    "src/auth/new_verifier.rs"
  ],
  "authorize": true
}
```

- **Target Path Specs**: Supports URI fragment syntax (`#symbol`) and line range slicing (`:start-end`).
- **Deferred Pre-Authorization**: When invoked during `Plan` stage with `authorize: true`, valid paths are queued in `deferred_authorized_paths` and automatically authorized upon plan approval (`workflow_gate(action="approve_plan", user_confirmed=true)`), cutting tool call overhead by 50%.
- **New File Declarations**: Files pending creation are marked `[NEW FILE - Pending Creation]` and checked against plural container suffixes (`*services`, `*utils`, etc.) for non-blocking `DOMAIN_COHESION_ADVISORY` hints.

### Subgraph Bundling with Direct Authorization (`subgraph_bundle`)

Packs target symbol definition, 1-hop inbound callers, and 1-hop outbound callees into a single token-bounded context snippet (< 250 LOC) with one-turn edit authorization:

```json
{
  "operation": "subgraph_bundle",
  "project_path": "/absolute/path/to/project",
  "target_symbol": "src/auth/token_service.rs#verify_token",
  "loc_budget": 250,
  "authorize": true
}
```
- **Target Path Spec**: Supports `path#symbol` directly in `target_symbol` or `query`.
- **Authorization Scope**: Authorizes target file by default; pass `authorize_all: true` to include all callers and callees.
- **Stage Alignment**: Authorizes directly in `Build` stage; queues into `deferred_authorized_paths` in `Plan` stage.

### Session-Bounded AST Response Cache (Zero-Cost Repeated Reads)

To prevent redundant Tree-sitter AST parsing and filesystem stat roundtrips when reviewing code across multiple turns:
- **30-Second TTL**: Caches rendered bounded AST slices, zoom reads, and code skeletons in memory for 30 seconds.
- **Dual Invalidation Strategy**: Validates TTL alongside zero-cost filesystem `mtime` checks (`fs::metadata(path).modified()`). If any external process, IDE editor, or Git command modifies the file on disk, the cache is instantly bypassed.
- **Event-Driven Eviction**: File modification signals within the agent workflow (`record_modified_file`, `drain_dirty_files`) immediately evict cached slices for affected files.
- **Transparent Telemetry**: Cache hits are transparently annotated with `[ag: AST Response Cache Hit (freshness: Xs/30s)]` while always executing fresh edit authorizations (`helpers_auth`) and updating rollback snapshots.

### Call Graph & Blast Radius Analysis

Find all functions calling `verify_token`:
```json
{
  "operation": "callers",
  "project_path": "/absolute/path/to/project",
  "query": "verify_token"
}
```

Calculate blast radius and generate Mermaid DAG:
```json
{
  "operation": "blast_radius",
  "project_path": "/absolute/path/to/project",
  "query": "TokenService"
}
```

### LSP Goto Definition

```json
{
  "operation": "definition",
  "project_path": "/absolute/path/to/project",
  "relative_path": "src/main.rs",
  "query": "AuthMiddleware"
}
```

---

## Example 3: Workflow Governance & Stage Management (`workflow_gate`)

### Check Stage Status

```json
{
  "action": "status",
  "project_path": "/absolute/path/to/project"
}
```

### Approve Plan (Requires User Consent & Auto-Authorizes Plan Artifact)

When user approves the plan, calling `approve_plan` automatically discovers and extracts target files from `implementation_plan.md` (or custom `artifact_path`), combines them with any `relative_paths` or queued `deferred_authorized_paths`, and batch pre-authorizes them in a single turn:

```json
{
  "action": "approve_plan",
  "project_path": "/absolute/path/to/project",
  "user_confirmed": true,
  "justification": "Implement approved architecture changes"
}
```

### Authorize File Edit (Single File & Batch Authorization)

Authorize single files or batch related domain files together; when a file exceeds the 300 LOC soft limit or has high function complexity, AST quality and domain cohesion advisories are surfaced non-blockingly:

```json
{
  "action": "authorize_edit",
  "project_path": "/absolute/path/to/project",
  "relative_paths": ["src/auth/token_service.rs", "src/auth/token_validator.rs"],
  "risk_level": "LOW",
  "justification": "Refactor/Decompose token validation into focused sub-module"
}
```

### Transition Workflow Stage

```json
{
  "action": "set_stage",
  "project_path": "/absolute/path/to/project",
  "target_stage": "Test"
}
```

---

## Example 4: Session Memory & Continuity (`session_continuity`)

Save session progress across restarts:

```json
{
  "operation": "save",
  "project_path": "/absolute/path/to/project"
}
```

Record persistent architectural instinct or project rule:

```json
{
  "operation": "learn",
  "project_path": "/absolute/path/to/project",
  "learning": "Database migrations must execute inside an explicit SQL transaction.",
  "category": "domain_rule",
  "pinned": true
}
```

Generate cross-agent session handoff summary:

```json
{
  "operation": "handoff",
  "project_path": "/absolute/path/to/project",
  "next_action": "Run integration tests and update API documentation."
}
```

---

## Security Verification & Binary Provenance

Verify binary integrity, publisher identity, and platform digital signatures directly from the CLI:

```bash
agent-guidance --fingerprint
# or: agent-guidance --verify-signature
```

Outputs a structured provenance report:
- **Publisher**: Verified identity (`Jun Mystery <darkzeuslk@gmail.com>`)
- **Cryptographic Digest**: Pure-Rust SHA-256 binary hash
- **Target & Commit**: Build architecture and Git commit hash
- **Signature Status**: Platform digital signature validity (Windows Authenticode, macOS codesign, Linux GPG)

---

## Token Guidance & Best Practices

- **300 LOC Soft Limit & Function Complexity**: Treat 300 LOC as a soft cohesion warning rather than a hard blocker. Keep functions focused (15-40 LOC, warn > 60 LOC, cyclomatic <= 10, cognitive <= 15, nesting <= 4), organize by domain/feature, and run language static linters (`cargo clippy`, `eslint`, `ruff`, `golangci-lint`); tests, docs, data, configs, assets, and `.mdc` files are exempt.
- **Rule Precedence Hierarchy**: User's main rules (`AGENTS.md`, `RULES.md`, system prompt) always take precedence over injected MCP tool micro-rules.
- **Targeted Reads**: Prefer `target_symbol` or `start_line`/`end_line` ranges over whole-file dumps.
- **Tree Scans**: Use `max_depth=3` (default) for balanced project overview.
- **Avoid Duplication**: Search for existing utilities with `project_context(operation="reusable")` before adding new helper logic.

## Related Docs

- [MCP Surface Reference](reference/mcp-surface.md)
- [Dashboard Guide](dashboard.md)
- [Client Configuration](setup/client-configuration.md)
- [Architecture](ARCHITECTURE.md)
