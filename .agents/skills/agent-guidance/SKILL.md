---
name: agent-guidance
description: Core system standards check and token-optimized codebase context retrieval. Run this skill before performing any tool execution or codebase changes.
---

## When to use me
- At the start of every task, repository lookup, or phase transition (`plan -> build -> test -> review`).

## Execution Protocol (`agent-guidance` MCP)
1. **Init & Skills**: Call `task_pipeline(task, project_path, phase="plan")` FIRST (bundles skill discovery; skip redundant `guidance(search)`). When skills are proposed, confirm via `ask_question(..., is_multi_select=true)` in user's language before `select_skills(..., user_confirmed=true)`.
2. **Search & Bounded Read**: Use `project_context(operation="search" | "graph_rag" | "read" | "symbols")` exclusively (300-line read cap for token budget). Batch files via `relative_paths=[...]` and pass `authorize=true` when reading to edit. Never use native IDE or shell read/search tools.
3. **Edit Gate & Code Quality**: Batch-authorize via `workflow_gate(action="authorize_edit", relative_paths=[...])` or `approve_plan(user_confirmed=true)`. Treat 300 LOC as a soft cohesion warning, not a hard limit—keep cohesive files intact, prioritize small functions (15-40 LOC; warn > 60 LOC, cyclomatic > 10, cognitive > 15, nesting > 4), and organize by domain/feature.
4. **Verify & Test Isolation**: Reuse shared utilities (DRY); isolate tests from live user/prod data (temp dir / in-memory DB), prefer `patch.object`/DI with call assertions, guard production storage/backup writes, and verify via `guidance(operation="verify", ...)` plus static linters (`clippy`, `eslint`, `ruff`, `golangci-lint`).
