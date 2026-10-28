# MANDATORY AGENT EXECUTION PROTOCOL (GEMINI / ANTIGRAVITY)

CRITICAL OVERRIDE: This repository is managed by the `agent-guidance` MCP Server. Route all code search and context inspection through `agent-guidance` MCP tools (Token-Bounded Reads & GraphRAG) to prevent token bloat.

## Execution Lifecycle & Strict Rules
1. **Turn 1 Initialization & Skill Discovery**: Call `task_pipeline(task="<user request>", project_path="<abs_path>", phase="plan")` FIRST to initialize context, detect architecture pattern, and establish workspace boundaries. Immediately follow by calling `guidance(operation="search", query="<user request>")` to discover required domain skills.
   - [EXEMPTIONS & SLASH COMMANDS]:
     1. Slash Commands (`/<cmd>` e.g. `/plan`, `/grill-me`, `/automation`, `/generative_ui`): Slash commands do NOT auto-load skills. `/<cmd>` tokens are stripped from search queries. If query contains only slash commands, skill proposals are bypassed entirely. When `/generative_ui` is present, skill proposals and `ask_question` calls are completely bypassed.
     2. Mentioned Skills: When the user explicitly mentions a skill name in text, the agent receives it prioritized with `(Recommended)` at option 1 in the `ask_question` modal.
     3. Informational Questions: Pure Q&A queries bypass skill proposals; answer directly without `ask_question`.
   - [GENERATIVE UI PROTOCOL & STRICT ASK_QUESTION PROHIBITION]: When `/generative_ui` is active or designing UI alternatives, calling `ask_question` is STRICTLY PROHIBITED for any reason (skill selection, layout choices, interviews). The agent MUST create self-contained HTML artifact file(s) (`UserFacing: true`) and embed them inline via `<agent-embed src="file:///.../widget.html"></agent-embed>` directly without modal prompts (`BLIND_UI_CHOICE_PROHIBITED`, `GENERATIVE_UI_NO_ASK_QUESTION`).
   - [STANDARD PROPOSAL]: When `guidance` returns `SKILL_PROPOSAL: MANDATORY USER INTERACTION REQUIRED`, ask the user via `ask_question(questions=[{question: "...", options: [...], is_multi_select: true}])` (format options as `"{name} - {short_desc}"` with short `{short_desc}` <= 38 chars) before calling `select_skills`. Do NOT inspect codebase files or start planning before completing skill discovery.
2. **Token-Bounded Context & GraphRAG (Strict File Reading Protocol)**:
   - [PROHIBITION]: NEVER use native IDE tools (`view_file`, `grep_search`, `find_by_name`, `list_dir`) OR shell read commands (`run_command` with `Get-Content`, `cat`, `type`, `head`, `tail`, `sed`, `awk`, or Python/script file reading) to inspect or search codebase files.
   - [MANDATORY]: Inspect, search, and read code EXCLUSIVELY via `project_context(operation="search" | "graph_rag" | "read" | "symbols", ...)` (300 LOC cap). Native file dumps and shell-based content reads are strictly forbidden. Shell execution (`run_command`) is strictly reserved for builds, tests, running tools, and git commands.
   - [CLUSTERED READ MANDATE]: When inspecting multiple related files, ALWAYS use clustered read `project_context(operation="read", relative_paths=[...])` (or `operation="cluster_read"`) to read all relevant files in a single turn instead of issuing sequential single-file reads. Immediately follow up by authorizing the entire cluster in a single batch turn using `workflow_gate(action="authorize_edit", relative_paths=[...])`.
3. **Plan & Design (Grill-Me & Approval Separation)**:
   - [GRILL-ME PROTOCOL]: If user requested `/grill-me` or interactive interview, ask all design clarification questions via `ask_question` in separate interview turn(s) BEFORE creating `implementation_plan.md`.
   - [SIMULTANEOUS CALL PROHIBITION]: NEVER call `ask_question` and create/update `implementation_plan.md` in the same turn (`SIMULTANEOUS_PLAN_AND_QUESTION_PROHIBITED`: doing so renders a blocking question modal over the artifact, swallowing the Proceed button and hiding the plan).
   - [FALSE APPROVAL PROHIBITION]: User selecting options in `ask_question` is strictly design feedback, NOT plan approval (`FALSE_PLAN_APPROVAL_PROHIBITED`). The agent MUST synthesize decisions into `implementation_plan.md` (`RequestFeedback: true`), output a terse message, and STOP tool calls to wait for user to click `Proceed` or send explicit approval (`proceed`, `ok`, `approved`, `triển khai`).
   - Calling `workflow_gate(action="approve_plan", user_confirmed=true)` or editing code without explicit user plan confirmation (Proceed button or approval message) is strictly forbidden.
4. **Batch/Per-File Edit Authorization Gate, Modularity & 300 LOC Hard Cap**: Authorize all planned edits in a single batch call via `workflow_gate(action="authorize_edit", project_path="...", relative_paths=[...], risk_level="LOW", justification="...", architecture_pattern="Auto")` or directly during plan approval via `workflow_gate(action="approve_plan", user_confirmed=true, relative_paths=[...])`. [PROHIBITION]: Calling `authorize_edit` individually file-by-file across multiple turns when a file list is known upfront is strictly forbidden. Individual authorization (`relative_path="..."`) is reserved solely for ad-hoc single files discovered during development. All source code files MUST remain strictly < 300 LOC (target < 150 LOC per sub-module; refactor bypass keywords: `refactor`, `decompose`, `extract`, `split`, `tách file`; exempt: docs, markdown, data, configs, assets). Strict Single Responsibility: compound plural suffixes (e.g. `*services`, `*controllers`, `*utils` - 40 patterns) and multi-component justifications are prohibited (`COMPOUND_FILE_NAME_PROHIBITED`, `MULTI_COMPONENT_NEW_FILE_PROHIBITED`).
5. **Apply Surgical & Reusable Changes (DRY & Shared Code Mandate)**:
   - [PROHIBITION]: NEVER duplicate logic, helper functions, formatters, validators, or UI components that already exist.
   - [MANDATORY]: Always search for existing shared utilities (`shared/`, `utils/`, `common/`, `helpers/`) via `project_context` before writing new code.
   - [REUSE & UNIFY]: If an identical or highly similar function (>85% semantic match) already exists, import and reuse it. When creating cross-cutting logic, extract it into a unified shared module. Apply modular changes respecting the authorized architecture pattern.
6. **Empirical Verification**: Call `guidance(operation="verify", verification_command="...", expected_output_keyword="...")` and run automated tests.
7. **Background Tasks**: Stop tool calling immediately when a background command is launched and wait for system reactive wakeup.

## Quick Tool Reference
| Phase / Task | MCP Tool | Operation / Action |
|---|---|---|
| Turn 1 Init & Skills | `task_pipeline` / `guidance` / `select_skills` | `phase="plan"` / `operation="search"` / `skills=[...]` |
| GraphRAG & Code Search | `project_context` | `operation="search"` \| `"graph_rag"` |
| Token-Bounded File/Symbol Read | `project_context` | `operation="read"` (`relative_paths=[...]` \| `target_symbol="..."`) |
| Edit Authorization Gate | `workflow_gate` | `action="authorize_edit"` (`relative_paths=[...]` \| `relative_path="..."`) |
| Standards, Docs & Verification | `guidance` | `operation="search"` \| `"docs"` \| `"verify"` |
| Session Continuity & Memory | `session_continuity` | `operation="save"` \| `"load"` \| `"learn"` \| `"handoff"` |
