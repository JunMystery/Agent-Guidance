use serde_json::Value;

use crate::catalog::language_detector::detect_language_fast;
use crate::mcp::state::ServerState;
use super::helpers::{detect_project_architecture, detect_project_path};

pub(crate) fn handle_precode(
    arguments: &Value,
    query: &str,
    state: &mut ServerState,
) -> String {
    let proj_path_arg = arguments
        .get("project_path")
        .and_then(|p| p.as_str())
        .unwrap_or(".");
    let proj_path = detect_project_path(proj_path_arg, state);
    let profile = detect_language_fast(&proj_path, query);
    let active_arch = state
        .active_architecture_pattern
        .clone()
        .unwrap_or_else(|| detect_project_architecture(&proj_path));

    let primary_lang = if profile.primary_languages.contains("rust") {
        "Rust"
    } else if profile.primary_languages.contains("kotlin")
        || profile.primary_languages.contains("java")
    {
        "Kotlin/Java"
    } else if profile.primary_languages.contains("go") {
        "Go"
    } else if profile.primary_languages.contains("python") {
        "Python"
    } else if profile.primary_languages.contains("typescript")
        || profile.primary_languages.contains("javascript")
    {
        "TypeScript/JavaScript"
    } else {
        "General"
    };

    let (lang_rules, linter_hint) = match primary_lang {
        "Rust" => (
            "- Rust Safety: Explicit lifetime/borrowing checks, handle Result/Option cleanly, avoid unwrap() in production paths.",
            "cargo clippy --all-targets",
        ),
        "Kotlin/Java" => (
            "- Kotlin/Java Safety: Scope coroutines to Dispatchers.IO/Default, avoid forced unwraps (!!), respect StateFlow/LiveData lifecycles, keep Compose functions idempotent.",
            "ktlint / detekt",
        ),
        "Go" => (
            "- Go Safety: Enforce explicit error checking (if err != nil), bind goroutine lifecycles to context.Context cancellation, avoid data races on shared structs.",
            "golangci-lint run",
        ),
        "Python" => (
            "- Python Safety: Type hints, handle None dereferences explicitly, avoid mutable default arguments.",
            "ruff check .",
        ),
        "TypeScript/JavaScript" => (
            "- TS/JS Safety: Strict type definitions, optional chaining (`?.`), nullish coalescing (`??`).",
            "eslint .",
        ),
        _ => (
            "- Language Safety: Verify non-null objects before dereferencing, enforce explicit error handling.",
            "language static linter",
        ),
    };

    let arch_blueprint = match active_arch.as_str() {
        "Clean_Architecture" => {
            "- Domain/Feature Grouping: Colocate files by domain (`domain/`, `usecase/`, `infrastructure/`, `entrypoint/`) so changes stay localized."
        }
        "Layered_Architecture" => {
            "- Domain/Feature Grouping: Group cohesive use cases across `controllers/`, `services/`, `models/`."
        }
        "Package_By_Feature" => {
            "- Domain/Feature Grouping: Keep feature entry, handlers, service, and types colocated under `<feature>/`."
        }
        "CLI_Pipeline" => {
            "- Domain/Feature Grouping: Keep `main` as thin CLI dispatcher, group sub-commands in `commands/`, and core logic in `core/`."
        }
        "Flat_Library" => {
            "- Domain/Feature Grouping: Keep public API facade clean and group internal modules by domain."
        }
        _ => {
            "- Domain/Feature Grouping: Group files serving the same use case or domain close together to minimize cross-module side effects."
        }
    };

    format!(
        "# Pre-Code Verification Checklist\n\n- Primary Language: {}\n- Architecture Pattern: {}\n\n1. **Domain Cohesion & Soft Quality Thresholds**:\n   {}\n   - *Soft 300 LOC Limit*: Treat 300 LOC as a review prompt (is this file mixing unrelated responsibilities?), not a hard wall. Prioritize small functions (15-40 LOC; warn > 60 LOC, cyclomatic <= 10, cognitive <= 15, nesting <= 4).\n2. **Language Rules & Static Linter**:\n   {}\n   - *Static Analysis*: Run `{}` alongside tests to catch complexity and nesting early.\n3. **Symbol & API Grounding**:\n   - Verify symbol signatures using `project_context(operation=\"symbols\")` or search before modifying callers.\n4. **Error Handling Integrity**:\n   - Preserve existing error boundaries. Never use unwrap() or empty catch blocks in production paths.\n5. **Test Isolation & Data-Boundary Safety**:\n   - Isolate tests into temp/in-memory sandboxes (never write to live APPDATA/~/.config/prod DB); prefer `patch.object`/DI over string-path mocks with call assertions, and use narrow atomic writes + empty-payload guards on production storage/backups.",
        primary_lang, active_arch, arch_blueprint, lang_rules, linter_hint
    )
}
