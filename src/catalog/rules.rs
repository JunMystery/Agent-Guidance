//! Dynamic Context-Aware Rule Generator
//!
//! Integrates Karpathy's 4 core engineering principles (Think Before Coding, Simplicity First,
//! Surgical Changes, Goal-Driven Execution) alongside MCP architectural mandates.
//! Injects strictly relevant, targeted rules per lifecycle phase to prevent token bloat and agent confusion.

/// Returns targeted execution rules tailored to the active workflow phase.
pub fn get_phase_rules(phase: &str) -> &'static str {
    match phase.trim().to_lowercase().as_str() {
        "context" | "search" | "read" | "explore" => {
            "## Exploration & Context Mandates (Karpathy: Think Before Coding)\n\
            1. **No Silent Assumptions**: Verify codebase facts via `project_context` before touching code. If uncertain or ambiguous, surface tradeoffs and ask rather than guess.\n\
            2. **Token-Bounded Reads**: Use `target_symbol` or AST skeleton mode (max 300 lines per read).\n\
            3. **No Direct Raw FS**: Route file access through MCP tools rather than raw filesystem dumps.\n\
            4. **Generative UI No ask_question**: When `/generative_ui` is active, calling `ask_question` is STRICTLY PROHIBITED; render HTML artifacts and `<agent-embed>` directly."
        }
        "plan" | "architecture" | "design" | "init" => {
            "## Planning & Architecture Mandates (Karpathy: Simplicity First & Goal-Driven)\n\
            1. **Simplicity First**: Design the minimum code that solves the problem. No speculative abstractions, unrequested flexibility, or unused configurability.\n\
            2. **Domain/Feature Cohesion & Soft 300 LOC Threshold**: Group files serving the same domain/use case together to localize edits. Treat 300 LOC as a soft review limit (not a hard cap) and prioritize focused functions (15-40 LOC; warn > 60 LOC, cyclomatic <= 10, cognitive <= 15, nesting <= 4).\n\
            3. **Goal-Driven Milestones**: Define concrete verifiable criteria for each step (`1. [Step] -> verify: [check]`).\n\
            4. **Grill-me & Plan Approval Separation**: Complete `/grill-me` interviews via `ask_question` in separate turns BEFORE writing `implementation_plan.md`. NEVER call `ask_question` and `write_file(implementation_plan.md)` in the same turn (`SIMULTANEOUS_PLAN_AND_QUESTION_PROHIBITED`). Question answers are design inputs, NOT plan approvals (`FALSE_PLAN_APPROVAL_PROHIBITED`). Always present the final plan in a standalone turn with `RequestFeedback: true` and wait for user Proceed.\n\
            5. **Generative UI Strictly Prohibits ask_question**: When `/generative_ui` is active, calling `ask_question` is STRICTLY PROHIBITED for any reason. Immediately create self-contained HTML artifact(s) (`UserFacing: true`) and embed inline via `<agent-embed>`."
        }
        "build" | "skills" | "implement" | "code" => {
            "## Code Construction Mandates (Karpathy: Surgical Changes & Simplicity First)\n\
            1. **Surgical Changes & Domain Cohesion**: Keep changes focused within the target domain/feature. When a file exceeds 300 LOC (soft limit), ask whether it handles unrelated responsibilities—keep it intact if cohesive, or extract overly long/complex functions (> 60 LOC, cyclomatic > 10, cognitive > 15, nesting > 4).\n\
            2. **Batch Edit Authorization & Quality Metrics**: Authorize planned edits upfront via `workflow_gate(action=\"authorize_edit\", relative_paths=[...])`. Use AST complexity advisories to keep functions small and readable (15-40 LOC target).\n\
            3. **Data-Boundary & Error Safety**: Never use unwrap() or empty catch blocks in production paths. On config/backup persistence, prefer narrow partial updates, atomic temp+rename writes, and empty-payload guards; keep test state isolated from live user/prod data."
        }
        "fix" | "debug" | "bugfix" => {
            "## Defect Remediation & Debugging Mandates (Karpathy: Surgical Changes)\n\
            1. **Root-Cause Isolation**: Isolate minimal failing case or reproduction before modifying code.\n\
            2. **Surgical Fixes & Complexity Check**: Touch only what is broken. If the target function exceeds 60 LOC or nesting > 4, extract focused helpers alongside the fix; 300 LOC per file is a soft review threshold, not a blocker.\n\
            3. **Regression Prevention & Test Isolation**: Verify the fix via `guidance(operation=\"verify\")` alongside your language static linter (clippy, eslint, ruff, golangci-lint). Ensure tests run in isolated temp/in-memory sandboxes without touching live user/prod data."
        }
        "test" | "verify" | "test_recheck" => {
            "## Verification & Empirical Testing Mandates (Karpathy: Goal-Driven Execution)\n\
            1. **Empirical Verification**: Run real automated tests and static linters via `guidance(operation=\"verify\")` before claiming completion.\n\
            2. **Test Isolation & Mock Integrity**: Isolate tests from live user/prod data (temp dir / in-memory DB); prefer `patch.object` or DI over string-path mocks, assert side-effect mock calls, and verify live data remains unchanged.\n\
            3. **Documentation Integrity**: Keep comments and docs synchronized with code modifications."
        }
        "review" | "audit" | "refactor" | "decompose" => {
            "## Code Review & Refactoring Mandates (Karpathy: Simplicity First)\n\
            1. **Cohesion Over Arbitrary Line Splitting**: When a file exceeds 300 LOC (soft limit), ask whether it mixes unrelated responsibilities—if coherent and well-factored, keeping it intact is reasonable.\n\
            2. **Function Size & Static Complexity**: Prioritize functions of 15-40 LOC (flag > 60 LOC, cyclomatic > 10, cognitive > 15, nesting > 4) and verify with static linters.\n\
            3. **Domain/Feature Organization**: Group related use-case files close together to localize future changes and avoid cross-module side effects."
        }
        _ => {
            "## Core Execution Mandates (Karpathy-Aligned)\n\
            1. **Think Before Coding**: Inspect symbols via `project_context` before code modifications.\n\
            2. **Batch Gating & Domain Grouping**: Authorize edits via `workflow_gate` and organize modules by domain/feature.\n\
            3. **Function Clarity & 300 LOC Soft Limit**: Keep functions small (15-40 LOC, warn > 60) with shallow nesting (<= 4); treat 300 LOC as a soft cohesion warning."
        }
    }
}
