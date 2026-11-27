# MCP Surface Reference

[Back to README](../README.md)

Complete reference for every public MCP tool, resource, and prompt exposed by the Agent Guidance MCP server.

---

## Tools (6)

All code inspection, modification, and execution tools require `task_pipeline` to be called first. Gated tools return `PRIORITY_REQUIRED` if called before `task_pipeline`. Ungated tools are `workflow_gate`, `session_continuity`, and `select_skills`.

### 1. `task_pipeline` -- Call First (Unlocks Priority Gate)

One-stop context preparation, Turn 1 skill discovery, and priority gate unlocker. Initializes workspace boundaries, detects project architectural patterns, auto-bundles skill search proposals (`phase="plan"`), synthesizes domain/feature blueprints (300 LOC soft cohesion limit & AST complexity targets), and injects memorized project learnings in a single token-bounded response (eliminating a separate `guidance(operation="search")` roundtrip).

```python
task_pipeline(
    task: str,                         # required — description of work
    project_path: str,                 # required — absolute path of active working repository
    phase: str,                        # required — active development phase: "plan" | "build" | "implement" | "test" | "debug" | "review" | "refactor"
    is_minor: bool = False,            # optional — Minor Task Fast-Path Exemption (typos, comments, docs, single-file patches)
) -> str
```

**Returns formatted markdown containing:**
- `Task`, `Active Phase`, and `Project` summary (with total indexed file count).
- `Project Memorized Learnings` — semantically relevant past session learnings from `.agent-context/learnings.md`.
- `Dynamic Split Blueprint` — upfront domain/feature blueprint tailored to detected architecture (Clean Architecture, Layered Architecture, Package by Feature, CLI Pipeline, Flat Library, Orchestrator) and review candidates (`> 300 LOC`).
- `Architecture Guidance` & `Core Phase Rules` — targeted execution mandates for active phase.
- `Priority Gate: PASSED` — confirms unlock for subsequent MCP tool calls.
- `-> NEXT STEP` — clear directive guiding next tool execution.

**Minor Task Fast-Path Exemption:**
If `is_minor: true` (or if the task description matches minor task heuristics such as typos, comments, docs, readme, or small single-file patches), `task_pipeline` automatically bypasses skill proposal modals (`ask_question`) and formal `implementation_plan.md` artifact generation. It auto-advances the workflow stage directly to `Build` (`plan_approved = true`, `is_fast_path = true`), allowing immediate inspection and editing. Enforces a Strict Boundary Guard of at most 1 code file or 3 doc/config files.

**Auto-Plan-Approval Feature:**
When a prior `Plan` turn is active (or an explicit IDE artifact approval message such as `the user has approved this document` is passed) and `task` matches short approval phrases, `task_pipeline` sets `plan_approved = true` (without granting blanket `edit_authorized`) and transitions the active stage directly to `Build`.

**Example:**
```python
task_pipeline(task="Add JWT auth to Express API", project_path="E:/Github/MyApp", phase="plan")
```

---

### 2. `select_skills` -- Inject Selected Skills

Confirm which proposed or catalog skills to load into the active conversation context. Injects token-optimized markdown with smart passage preservation and dynamic token measurement. Standard skills under 250 LOC preserve their full compressed content without destructive slicing, while larger reference skills receive vector-ranked section slices (up to 5 sections, ~80% token savings) with YAML frontmatter and hollow parent headers filtered out. Pass skill names (e.g. `['android-clean-architecture']`), or pass an empty array `[]` to skip all.

In **Remote Client Mode**, `select_skills` transparently forwards the active task from `task_pipeline` to the Remote ML Worker (`POST /api/skills/slice`), receiving vector-ranked surgical section slices with local SQLite LRU caching in `usage.db`.

```python
select_skills(
    skills: list[str],                 # required — skill names to load (e.g. ["android-clean-architecture"]), or [] to skip
    task: str | None = None,           # optional active task description for semantic section slicing
    project_path: str | None = None,   # absolute path of active repository workspace
    user_confirmed: bool = False,      # user confirmation flag (mandatory when proposals are pending)
    user_message: str | None = None,   # optional message or feedback from user regarding skill selection
) -> str
```

**Returns formatted markdown containing:**
- `Skill Selection Confirmed (<N>)` or `Skill Selection: No skills selected. Proceeding directly to task execution.`
- Sliced, compressed skill markdown bodies (`### Skill: <name>`).
- `Language Safety Rules` — micro-guidelines detected for the project tech stack (< 1ms).
- Dynamic token accounting recorded in `~/.agent-guidance/usage.db`.

**Aliases:** Tool alias `select_skill`; argument alias `skill` (string or comma-separated).

**Example:**
```python
select_skills(
    skills=["android-clean-architecture"],
    task="Implement repository pattern and domain usecases",
    user_confirmed=True
)
```

---

### 3. `guidance` -- Standards & Skill Catalog

Standards catalog, 2-stage vector search, 629 embedded skills, pre-code architecture blueprints, and empirical verification contracts. Supports 9 operations.

```python
guidance(
    operation: str,                    # required — search|get|list|precode|verify|workflow|ui_ux|docs|reindex_skills
    query: str | None = None,          # search query / keyword for search, docs, ui_ux, precode
    identifier: str | None = None,     # skill name/path for get / docs, or workflow stage for workflow (alias: topic)
    topic: str | None = None,          # alias for identifier on get / docs / workflow
    project_path: str | None = None,   # absolute path of active repository workspace
    verification_command: str | None = None,    # required for verify: shell test command (e.g. 'cargo test')
    expected_output_keyword: str | None = None, # required for verify: expected success keyword (e.g. 'ok')
) -> str
```

#### Operations

| Operation | Required Args | Description |
|---|---|---|
| `search` | `query` | 2-stage Candle BERT vector search + Cross-Encoder re-ranking across skills catalog. Fast shallow language profile matching (< 2ms). |
| `get` | `identifier` \| `topic` | Retrieve compressed body of an embedded, staged (`~/.agent-guidance/staging/skills/`), or local workspace skill (`.agents/skills`) via `resolve_skill` with markdown path/slug lookup. |
| `list` | -- | List all registered skills in embedded catalog and workspace (`.agents/skills`). |
| `docs` | `query` \| `identifier` | Search technical documentation cheat-sheets with token-bounded markdown slicing (< 400 tokens). |
| `workflow` | `identifier` (stage) | Load stage workflow guide (`plan`, `context`, `build`, `test`, `review`). |
| `precode` | `query` | Structured pre-code checklist (Domain Cohesion, 300 LOC Soft Limit, AST Complexity Targets & Static Linter Hints, Language Rules, Symbol Grounding, Error Boundaries). |
| `verify` | `verification_command`, `expected_output_keyword` | Register empirical verification contract to satisfy anti-hallucination mandate. |
| `ui_ux` | `query` | Retrieve UI/UX guidelines and modern design standards. |
| `reindex_skills` | -- | Force refresh semantic vector index for skills and rebuild local passage cache (`~/.agent-guidance/passage_vectors.bin`). (Alias: `reindex`). |

**Examples:**
```python
guidance(operation="search", query="jwt auth express")
guidance(operation="get", identifier="android-clean-architecture")
guidance(operation="docs", query="rust testing", identifier="rust-patterns")
guidance(operation="precode", query="android kotlin UI")
guidance(operation="verify", verification_command="cargo test --bin agent-guidance", expected_output_keyword="test result: ok")
```

---

### 4. `project_context` -- Code Graph, AST & Semantic Search Engine

Read, search, navigate code graphs, and explore project files with built-in token budgets (300-line token read budget), AST analysis, and persistent SQLite storage.

```python
project_context(
    operation: str,                    # required — see operations below
    project_path: str,                 # required — absolute path of active repository workspace
    query: str | None = None,          # search query / natural language / symbol name
    relative_path: str | None = None,  # relative file path for read, symbols, structure, learn_alias
    relative_paths: list[str] | None = None, # multiple file specs for clustered read ('path#symbol', 'path:10-50')
    authorize: bool = False,           # one-turn edit authorization (instant in Build, deferred in Plan)
    target_symbol: str | None = None,  # precise function/struct/class symbol to extract
    loc_budget: int = 250,             # LOC budget for subgraph_bundle (default: 250, clamped: 50..=500)
    max_depth: int = 3,                # max directory depth for tree operation (default: 3, max: 5)
    start_line: int | None = None,     # 1-indexed start line for read slice
    end_line: int | None = None,       # 1-indexed end line for read slice (max 300 LOC range)
    view_mode: str = "full",           # read mode: "full" | "skeleton" | "zoom" | "slice"
    mode: str = "drift",               # GraphRAG query mode: "global" | "local" | "drift" | "basic"
    scope: str = "all",                # scope for navigate: "symbols" | "files" | "edges" | "content"
    layer: str | None = None,          # architecture domain layer ("ui" | "domain" | "data" | "infrastructure")
    alias_term: str | None = None,     # natural language term for learn_alias
    resolved_symbol: str | None = None,# symbol name for learn_alias
    resolved_line: int | None = None,  # line number for learn_alias
    edges: list[dict] | None = None,   # semantic edges for enrich_graph: [{source, target, relation, description, confidence}]
    summaries: list[dict] | None = None,# domain summaries for enrich_graph: [{module_path, title, summary, tags}]
    allow_cross_workspace: bool = False,# explicitly permit reading/authorizing files outside active workspace root
) -> str
```

#### Operations

| Operation | Required Args | Description |
|---|---|---|
| `search` | `query` | 6-Phase Instant Cascade (<100ms): Alias Cache (<1ms) → Symbol FTS5 (<5ms) → Content FTS5 (<5ms) → Symbol Vectors (<50ms) → RAG Content Vectors (<100ms) → Linked Sibling Projects. Blends multi-result hits across cascade, bypasses alias cache short-circuiting on code syntax queries, and restricts auto-learning to natural multi-word phrases. |
| `subgraph_bundle` | `target_symbol` (or `query` / `symbol`) | Single-turn multi-file context pack: Target symbol definition + 1-hop caller snippets + 1-hop callee snippets under token budget (default 250 LOC, clamped 50..=500). Supports URI spec syntax (`path#symbol`), direct/deferred edit authorization (`authorize=True`), and full dependency authorization (`authorize_all=True`). (Alias: `bundle`). |
| `tree` | -- | Repository directory tree scan. Accepts `max_depth` (default: 3, range: 1..=5). |
| `read` | `relative_path` \| `relative_paths` | Bounded file read enforcing a 300-line token read budget (distinct from the file-size soft limit), bypassing markdown compression to preserve source formatting, and blocking external absolute paths by default (`allow_cross_workspace=False`). In single-file or multi-file cluster mode, supports universal Target Path Specs (`path#symbol`, `path:start-end`), pending new file declarations (`[NEW FILE - Pending Creation]`), direct/deferred edit authorization (`authorize=True`), and Session-Bounded AST Response Caching (30s TTL + disk `mtime` invalidation). Supports dedicated aliases `cluster_read`, `read_cluster`, `read_and_authorize`, and `read_auth`. |
| `symbols` | `relative_path` | Extract functions, structs, enums, classes, and traits across 6+ languages (Alias: `structure`). |
| `references` | `query` | Instant symbol usage lookup across codebase (<5ms SQLite FTS5 index). |
| `callers` | `query` | Graph query traversing AST call edges to identify all functions/modules that call target symbol (Alias: `incoming_calls`). |
| `callees` | `query` | Graph query traversing AST call edges to identify all functions/dependencies called by target symbol (Alias: `outgoing_calls`). |
| `blast_radius` | `query` | Calculates logarithmic change risk score and renders Mermaid LR diagram of downstream blast radius (Alias: `impact`). |
| `definition` | `query`, `relative_path` | LSP goto-definition with fallback to SQLite symbol index (Alias: `goto_definition`). |
| `type_definition` | `query`, `relative_path` | LSP type definition lookup with fallback to symbol table. |
| `graph_rag` | `query` | Hierarchical Leiden community GraphRAG synthesizing macro community summaries with local micro-retrieval (`mode="global" \| "local" \| "drift"`). |
| `reusable` | -- | AST & Graph analysis detecting duplicate implementations (>85% semantic match) and candidates for shared modules (Aliases: `shared`, `reusable_candidates`, `detect_duplicates`). |
| `architecture` | -- | Detects and generates Mermaid layer flowchart of project architecture pattern. |
| `navigate` | `query` | Semantic vector graph traversal across symbols, chunks, and edges simultaneously. |
| `learn_alias` | `alias_term`, `relative_path` | Store natural language alias mapping in SQLite with automated 30/90-day decay. |
| `reindex` | -- | Force full AST re-parse and queue background Multilingual-E5 vector embedding for symbols and chunks. |
| `enrich_graph` | `edges` \| `summaries` | Push agent-discovered semantic edges and domain summaries into `.agent-context/code_graph.db` (Alias: `enrich`). |
| `semantic_query`| `query` | Query agent domain summaries and semantic edges via vector search (Alias: `semantic`). |
| `data_flow` | `query` | Bidirectional intra-procedural taint flow and dynamic dispatch trace backed by read-only `code_graph.db` queries (Aliases: `dataflow`, `taint`, `flow`). |

**Examples:**
```python
project_context(operation="tree", project_path="E:/Github/MyApp", max_depth=3)
project_context(operation="subgraph_bundle", project_path="E:/Github/MyApp", target_symbol="src/service.rs#handle_request", loc_budget=250, authorize=True)
project_context(operation="search", project_path="E:/Github/MyApp", query="handle_api_stats")
project_context(operation="callers", project_path="E:/Github/MyApp", query="scan_project")
project_context(operation="callees", project_path="E:/Github/MyApp", query="handle_request")
project_context(operation="blast_radius", project_path="E:/Github/MyApp", query="ServerState")
project_context(operation="read", project_path="E:/Github/MyApp", relative_path="src/main.rs", start_line=1, end_line=120)
project_context(operation="read", project_path="E:/Github/MyApp", relative_path="src/router.rs", target_symbol="handle_request")
project_context(operation="read", project_path="E:/Github/MyApp", relative_path="src/router.rs", view_mode="zoom", target_symbol="dispatch")
project_context(operation="read", project_path="E:/Github/MyApp", relative_paths=["src/model.rs#User", "src/auth.rs:1-50", "src/new_service.rs"], authorize=True)
project_context(operation="graph_rag", project_path="E:/Github/MyApp", query="authentication lifecycle", mode="drift")
```

---

### 5. `workflow_gate` -- Governance & Stage State Machine

Manage active workflow stages, authorize file edits (AST complexity & 300 LOC soft cohesion advisories, domain cohesion checks), approve implementation plans, record test verifications, or restore rollback snapshots.

```python
workflow_gate(
    action: str,                       # required — check|status|set_stage|set_architecture|authorize_edit|advance|rollback|approve_plan|pass_verification|invalidate|sync_file
    project_path: str | None = None,   # working repository path
    relative_path: str | None = None,  # specific file to authorize edit on (required for authorize_edit)
    relative_paths: list[str] | None = None, # batch files to pre-authorize during approve_plan or authorize_edit
    artifact_path: str | None = None,  # optional custom path to implementation_plan.md for Server-Side Auto-Auth
    target_stage: str | None = None,   # target stage for set_stage / advance: Context|Plan|Ask_Revise|Build|Test_Recheck|Fix|Proposal|Review
    risk_level: str = "LOW",           # LOW | MEDIUM | HIGH (for authorize_edit / advance)
    justification: str | None = None,  # reason & test mitigation (mandatory for files with >8 dependents)
    architecture_pattern: str = "Auto",# Auto | Clean_Architecture | Layered_Architecture | Package_By_Feature | Orchestrator | CLI_Pipeline | Flat_Library
    user_confirmed: bool = False,      # user confirmation flag (required for approve_plan)
    user_message: str | None = None,   # evaluate message for approval keywords
    allow_cross_workspace: bool = False, # explicitly permit editing outside primary workspace root
    verification_output: str | None = None, # command output matched against expected_output_keyword in pass_verification
) -> str
```

#### Actions

| Action | Description |
|---|---|
| `check` | Check current stage and evaluate `user_message` for approval keywords. |
| `status` | Display full workflow state: active stage, plan approval flag, fix attempt count, edit authorization status. |
| `set_stage` | Manually transition workflow stage (`Context`, `Plan`, `Ask_Revise`, `Build`, `Test_Recheck`, `Fix`, `Proposal`, `Review`). |
| `set_architecture` | Detect, lock, and persist architectural pattern in `.agent-context/architecture.json`. |
| `authorize_edit` | Authorize editing a file. Attaches non-blocking AST quality advisories (`SOFT_FILE_LOC_LIMIT = 300`, `MAX_FUNCTION_LOC = 60`, `MAX_CYCLOMATIC = 10`, `MAX_COGNITIVE = 15`, `MAX_NESTING_DEPTH = 4`), `DOMAIN_COHESION_ADVISORY`, and cross-directory co-change hints, triggers zero-turn transition (`Plan -> Build` if approved), creates rollback snapshot, and unlocks write permission. |
| `advance` | Composite check, approval, and stage transition in a single call. Emits non-blocking post-edit quality and AST complexity advisories with static linter hints when advancing to `Proposal`/`Review`. (Alias: `advance_stage`). |
| `approve_plan` | Record explicit plan approval (requires `user_confirmed=True` or approval keyword), clear Fast-Path flags, and transition to `Build`. Supports **Server-Side Auto-Auth**: automatically extracts target files from `implementation_plan.md` (or custom `artifact_path`), combines them with `relative_paths` and queued `deferred_authorized_paths`, and batch pre-authorizes them in 1 turn with soft domain cohesion and AST quality advisories. (Alias: `approve`). |
| `pass_verification` | Validate `verification_output` against registered `expected_output_keyword` (when set), record empirical verification pass, and reset fix attempt counter. |
| `invalidate` | Write-through AST re-index for a specific `relative_path` after modification (rejecting `..` or NUL paths). (Alias: `sync_file`). |
| `rollback` | Restore pre-edit file snapshots from `.agent-context/snapshots/{session_id}/<rel_path>` (preserving relative directory structure, no `.snapshot` suffix) and reset `verification_passed` and `modified_files`. |

#### Stage Lifecycle
$$\text{Context} \longrightarrow \text{Plan} \longrightarrow \text{Ask\_Revise} \longrightarrow \text{Build} \longrightarrow \text{Test\_Recheck} \longrightarrow \text{Fix} \longrightarrow \text{Proposal / Review}$$

* **Build Gate**: Transitioning to `Build` or editing files requires `plan_approved = true`.
* **Fix Circuit Breaker**: Exceeding 3 consecutive fix attempts trips the circuit breaker, resetting stage to `Ask_Revise`, resetting `plan_approved = false`, and requiring user intervention.
* **Domain Cohesion Advisory (New Files)**: Emits non-blocking `DOMAIN_COHESION_ADVISORY` warnings when a new file uses plural container suffixes (`*utils`, `*helpers`, `*services`, etc.) or multi-component justifications to encourage grouping by domain/feature.
* **300 LOC Soft Limit & Built-in AST Complexity Analyzer**: Emits non-blocking warnings when a non-exempt file exceeds `SOFT_FILE_LOC_LIMIT = 300` or any function exceeds `MAX_FUNCTION_LOC = 60` (target 15-40), `MAX_CYCLOMATIC = 10` (flat `match`/`switch` arms = `+1`), `MAX_COGNITIVE = 15` (flat `else if` chains and `condition` nodes kept at parent nesting depth), or `MAX_NESTING_DEPTH = 4`. Exempts dedicated test paths, `#[cfg(test)]`/`#[test]`, docs, configs, data, assets, and `.mdc` rule files (`gate_exemptions.rs`).
* **Rule Precedence Hierarchy**: User's main rules (`AGENTS.md`, `RULES.md`, system prompt) strictly take precedence over injected tool micro-rules. Tool micro-rules appear as `[!NOTE]` advisories.
* **Critical Hub Guard**: Files with $> 8$ dependents require `justification` with $\ge 10$ characters.

**Examples:**
```python
workflow_gate(action="status", project_path="E:/Github/MyApp")
workflow_gate(action="approve_plan", project_path="E:/Github/MyApp", user_confirmed=True)
workflow_gate(action="authorize_edit", project_path="E:/Github/MyApp", relative_path="src/auth.rs", risk_level="LOW", justification="Add JWT token parser")
workflow_gate(action="set_stage", project_path="E:/Github/MyApp", target_stage="Test_Recheck")
workflow_gate(action="pass_verification", project_path="E:/Github/MyApp")
workflow_gate(action="rollback", project_path="E:/Github/MyApp")
```

---

### 6. `session_continuity` -- Session State & Memory

Save, restore, switch, and summarize cross-session agent context with zero-trust permission resets.

```python
session_continuity(
    operation: str,                    # required — save | load | switch | list | diff | learn | handoff | clear
    project_path: str,                 # required — absolute path of active repository workspace
    session_id: str | None = None,     # session ID to switch to (for switch)
    learning: str | None = None,       # knowledge, insight, or rule to memorize (for learn)
    category: str | None = None,       # category tag: "build_test" | "environment" | "architecture" | "domain_rule" | "general" (for learn)
    pinned: bool = False,              # pin learning item to protect from FIFO eviction (for learn)
    next_action: str | None = None,    # recommended next action for incoming agent (for handoff)
) -> str
```

#### Operations

| Operation | Description |
|---|---|
| `save` | Persist active conversation memory, current stage, modified files, and session metadata into `.agent-context/sessions/session_{pid}_{timestamp}.json`. |
| `load` | Load a specific session by `session_id`. Resets all authorization, verification, fast-path, and architecture flags to Zero-Trust state. |
| `switch` | Atomic handoff to an existing session (preserving active session if target is missing) with Zero-Trust permission, verification, and fast-path flag reset. |
| `list` | List all saved session checkpoints with timestamps, stage, notes, and pinned status (Alias: `sessions`). |
| `diff` | Generate unified change summary with accurate line deltas by comparing modified files against pre-edit snapshots in `.agent-context/snapshots/{session_id}/<rel_path>` (Alias: `changes`). |
| `learn` | Atomically record permanent cross-session project rule or architectural instinct into `.agent-context/learnings.md` with category and optional pinned status. |
| `handoff` | Atomically write structured session handoff document summarizing modified files, snapshot deltas, test status, and `next_action`. |
| `clear` | Remove unpinned session checkpoints and purge rollback snapshots. |

**Examples:**
```python
session_continuity(operation="save", project_path="E:/Github/MyApp")
session_continuity(operation="learn", project_path="E:/Github/MyApp", learning="Dashboard components must remain strictly under 120 LOC", category="architecture", pinned=True)
session_continuity(operation="list", project_path="E:/Github/MyApp")
session_continuity(operation="diff", project_path="E:/Github/MyApp")
session_continuity(operation="handoff", project_path="E:/Github/MyApp", next_action="Auth completed. Next step: frontend login modal")
```

---

## Domain Error Codes

The MCP server returns standardized domain error codes in response text when execution is blocked by safety gates:

| Error Code | Source Tool | Cause | Resolution |
|---|---|---|---|
| `PRIORITY_REQUIRED` | All gated tools | Tool invoked before `task_pipeline` | Call `task_pipeline(task="...", project_path="...", phase="plan")` first. |
| `WORKFLOW_STAGE_BLOCKED` | `workflow_gate` | Attempted file edit or stage transition without meeting stage prerequisites | Check `workflow_gate(action="status")`, approve plan with `approve_plan`, and advance to `Build`. |
| `USER_APPROVAL_REQUIRED` | `workflow_gate` | `approve_plan` called without `user_confirmed=True` | Obtain explicit user approval and invoke `workflow_gate(action="approve_plan", user_confirmed=True)`. |
| `USER_CONFIRMATION_REQUIRED` | `select_skills` | Skill selection invoked while proposals were pending without confirmation | Pass `user_confirmed=True` after user confirms selection. |
| `RELATIVE_PATH_REQUIRED` | `workflow_gate` | `authorize_edit` called without file path | Pass exact relative file path in `relative_path`. |
| `PATH_TRAVERSAL_PROHIBITED` | `workflow_gate` | Path escapes workspace root via `..` or invalid symlink | Specify relative path inside project boundaries. |
| `HIGH_RISK_JUSTIFICATION_REQUIRED` | `workflow_gate` | Modifying a critical hub file (>8 dependents) with justification < 10 chars | Provide detailed rationale and test verification plan in `justification`. |
| `TEST_SAFETY_VIOLATION` | `guidance(verify)`, `workflow_gate(advance)` | Modified test has mock namespace/path mismatch (`TEST_MOCK_NAMESPACE_MISMATCH`) or `verification_command` targets prod/destructive state (`VERIFY_COMMAND_PROD_TARGET`) | Switch to `patch.object`/DI or exact imported module path, target temp/in-memory sandbox, or add `test-safety-allow: <rule>` when explicitly verified. |

> **Non-Blocking Quality & Test-Safety Advisories (v2.0.2+)**: Replaced the former hard-block error codes (`300_LOC_CAP_EXCEEDED`, `COMPOUND_FILE_NAME_PROHIBITED`, `MULTI_COMPONENT_NEW_FILE_PROHIBITED`) with non-blocking `DOMAIN_COHESION_ADVISORY`, `Post-Edit Quality & Complexity Advisory (Soft Limit)`, and data-boundary advisories (`TEST_UNSANDBOXED_REAL_STATE`, `TEST_UNSANDBOXED_APP_INIT`, `TEST_UNVERIFIED_SIDE_EFFECT_MOCK`, `PERSISTENCE_OVERWRITE_SAFETY`) surfaced by `workflow_gate` and `guidance(operation="verify")`.

---

## Resources (5 Static + 1 Dynamic)

Resources provide direct read-only access to system status, embedded reference guides, and skill content:

1. `agent-guidance://system/edit-allowed` — Read-only JSON resource returning whether file editing is authorized (`stage == "Build" && plan_approved && edit_authorized`).
2. `standards://version` — JSON object containing server version and engine metadata.
3. `standards://manifest` — JSON index of embedded standards and skill catalog metadata.
4. `agent-guidance://system/priority` — Priority gate instructions returned when `PRIORITY_REQUIRED` occurs (also supports legacy alias `agent-guidance-mcp://system/priority`).
5. `agent-guidance://system/gate` — JSON status of the priority gate and sentinel file presence (also supports legacy alias `agent-guidance-mcp://system/gate`).
6. `standards://skill/{name}` (Dynamic) — Direct read access to full uncompressed markdown body of any embedded skill (e.g. `standards://skill/rust-patterns`).
