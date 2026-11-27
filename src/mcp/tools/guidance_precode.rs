use std::path::Path;
use serde_json::Value;

use crate::catalog::language_detector::{contains_word, detect_language_fast};
use crate::mcp::state::ServerState;
use super::helpers::{detect_project_architecture, detect_project_path};

fn detect_target_languages(arguments: &Value, query: &str) -> Vec<&'static str> {
    let mut langs: Vec<&'static str> = Vec::new();
    let mut push_lang = |lang: &'static str| {
        if !langs.contains(&lang) {
            langs.push(lang);
        }
    };

    let files_val = arguments
        .get("files")
        .or_else(|| arguments.get("relative_paths"))
        .or_else(|| arguments.get("relative_path"))
        .or_else(|| arguments.get("file_path"));

    let mut file_strs: Vec<String> = Vec::new();
    if let Some(arr) = files_val.and_then(|v| v.as_array()) {
        for item in arr {
            if let Some(s) = item.as_str() {
                file_strs.push(s.to_string());
            }
        }
    } else if let Some(s) = files_val.and_then(|v| v.as_str()) {
        file_strs.push(s.to_string());
    }

    for f in &file_strs {
        let ext = Path::new(f.trim())
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "rs" => push_lang("Rust"),
            "kt" | "kts" | "java" => push_lang("Kotlin/Java"),
            "go" => push_lang("Go"),
            "py" | "pyw" => push_lang("Python"),
            "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" => push_lang("TypeScript/JavaScript"),
            _ => {}
        }
    }

    let framework_str = arguments
        .get("framework")
        .or_else(|| arguments.get("tech_stack"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let hint_text = format!("{} {}", query, framework_str).to_lowercase();

    for kw in ["rust", "cargo", "axum", "actix", "tokio"] {
        if contains_word(&hint_text, kw) {
            push_lang("Rust");
            break;
        }
    }
    for kw in ["kotlin", "java", "android", "compose", "spring", "springboot", "gradle"] {
        if contains_word(&hint_text, kw) {
            push_lang("Kotlin/Java");
            break;
        }
    }
    for kw in ["go", "golang", "gin", "echo", "fiber"] {
        if contains_word(&hint_text, kw) {
            push_lang("Go");
            break;
        }
    }
    for kw in ["python", "django", "fastapi", "flask", "pytest", "pytorch"] {
        if contains_word(&hint_text, kw) {
            push_lang("Python");
            break;
        }
    }
    for kw in [
        "typescript", "javascript", "react", "vue", "angular", "svelte", "next", "nextjs",
        "node", "nodejs", "vite", "tailwind",
    ] {
        if contains_word(&hint_text, kw) {
            push_lang("TypeScript/JavaScript");
            break;
        }
    }

    langs
}

fn lang_rules_and_linter(lang: &str) -> (&'static str, &'static str) {
    match lang {
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
    }
}

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
    let active_arch = state
        .active_architecture_pattern
        .clone()
        .unwrap_or_else(|| detect_project_architecture(&proj_path));

    let mut target_langs = detect_target_languages(arguments, query);
    if target_langs.is_empty() {
        let profile = detect_language_fast(&proj_path, query);
        if profile.primary_languages.contains("rust") {
            target_langs.push("Rust");
        }
        if profile.primary_languages.contains("kotlin") || profile.primary_languages.contains("java") {
            target_langs.push("Kotlin/Java");
        }
        if profile.primary_languages.contains("go") {
            target_langs.push("Go");
        }
        if profile.primary_languages.contains("python") {
            target_langs.push("Python");
        }
        if profile.primary_languages.contains("typescript") || profile.primary_languages.contains("javascript") {
            target_langs.push("TypeScript/JavaScript");
        }
    }
    if target_langs.is_empty() {
        target_langs.push("General");
    }

    let primary_lang = target_langs.join(" + ");
    let mut rules_vec = Vec::new();
    let mut linters_vec = Vec::new();
    for lang in &target_langs {
        let (rules, linter) = lang_rules_and_linter(lang);
        rules_vec.push(rules);
        if !linters_vec.contains(&linter) {
            linters_vec.push(linter);
        }
    }
    let lang_rules = rules_vec.join("\n   ");
    let linter_hint = linters_vec.join(" ; ");

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
