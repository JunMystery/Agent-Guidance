//! Dynamic Context-Aware Rule Generator
//!
//! Integrates Karpathy's 4 core engineering principles (Think Before Coding, Simplicity First,
//! Surgical Changes, Goal-Driven Execution) alongside MCP architectural mandates.
//! Injects strictly relevant, targeted rules per lifecycle phase to prevent token bloat and agent confusion.

/// Returns targeted execution rules tailored to the active workflow phase.
pub fn get_phase_rules(phase: &str) -> &'static str {
    match phase.trim().to_lowercase().as_str() {
        "context" | "search" | "read" | "explore" => {
            "## Exploration & Context Mandates\n\
            - Verify facts via `project_context` (bounded <=300L/read); no raw FS dumps.\n\
            - `/generative_ui`: NEVER call `ask_question`; emit HTML artifact + `<agent-embed>`."
        }
        "plan" | "architecture" | "design" | "init" => {
            "## Planning & Architecture Mandates\n\
            - Minimum code solving the task; group by domain, keep fns 15-40L (warn >60L, cyc <=10, cog <=15, nest <=4), 300L soft limit.\n\
            - Complete `/grill-me` `ask_question` turns BEFORE `implementation_plan.md` (`SIMULTANEOUS_PLAN_AND_QUESTION_PROHIBITED`, `FALSE_PLAN_APPROVAL_PROHIBITED`).\n\
            - `/generative_ui`: NEVER call `ask_question`; emit HTML artifact (`UserFacing: true`) + `<agent-embed>`."
        }
        "build" | "skills" | "implement" | "code" => {
            "## Code Construction Mandates\n\
            - Batch `workflow_gate(action=\"authorize_edit\", relative_paths=[...])`; keep fns <=60L (300L soft cohesion limit).\n\
            - No `unwrap()` or empty catch in prod; atomic writes + empty-payload guards; isolate test data."
        }
        "fix" | "debug" | "bugfix" => {
            "## Defect Remediation Mandates\n\
            - Isolate root cause; touch only broken code (extract helpers if >60L or nest >4).\n\
            - Verify via `guidance(operation=\"verify\")` + linter in isolated temp/in-memory sandbox."
        }
        "test" | "verify" | "test_recheck" => {
            "## Verification Mandates\n\
            - Run real tests + static linters via `guidance(operation=\"verify\")`; isolate tests from live user/prod data.\n\
            - Prefer `patch.object`/DI over string-path mocks with call assertions."
        }
        "review" | "audit" | "refactor" | "decompose" => {
            "## Code Review & Refactoring Mandates\n\
            - Keep cohesive >300L files intact; split only mixed domains or complex fns (>60L, cyc >10, cog >15, nest >4).\n\
            - Group related domain files together; verify with static linters."
        }
        _ => {
            "## Core Execution Mandates\n\
            - Inspect via `project_context`, authorize via `workflow_gate`, keep fns <=60L (300L soft limit)."
        }
    }
}
