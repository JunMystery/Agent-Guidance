//! Minor Task Fast-Path Exemption analyzer and payload generator.
//! Detects minor tasks (typos, comments, docs, single-file patches) to bypass
//! skill proposal and plan approval ceremonies, auto-advancing to Build stage.

use std::path::Path;
use serde_json::Value;

use crate::catalog::language_detector::contains_word;

fn contains_any_word(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| contains_word(haystack, needle))
}

pub fn infer_or_normalize_phase(raw_phase: Option<&str>, task: &str) -> String {
    let raw = raw_phase.unwrap_or("").trim().to_lowercase();
    match raw.as_str() {
        "build" | "implement" | "code" | "create" => return "build".to_string(),
        "test" | "verify" | "test_recheck" | "testing" => return "test".to_string(),
        "debug" | "fix" | "bugfix" | "patch" => return "fix".to_string(),
        "review" | "audit" | "proposal" => return "review".to_string(),
        "refactor" | "decompose" => return "refactor".to_string(),
        _ => {}
    }

    let task_lower = task.trim().to_lowercase();
    if contains_any_word(
        &task_lower,
        &["plan", "planning", "kế hoạch", "design", "thiết kế", "architecture", "kiến trúc"],
    ) {
        return "plan".to_string();
    }

    if contains_any_word(
        &task_lower,
        &[
            "fix", "fixed", "fixing", "bug", "bugs", "bugfix", "crash",
            "sửa lỗi", "khắc phục", "vấn đề", "error", "errors", "issue", "issues",
        ],
    ) {
        return "fix".to_string();
    }

    if contains_any_word(
        &task_lower,
        &["test", "tests", "testing", "kiểm thử", "verify", "assert", "kiem thu"],
    ) {
        return "test".to_string();
    }

    if contains_any_word(
        &task_lower,
        &["review", "audit", "đánh giá", "soát code", "soát lỗi"],
    ) {
        return "review".to_string();
    }

    if contains_any_word(
        &task_lower,
        &["refactor", "refactoring", "tách file", "decompose", "clean up", "cleanup", "tối ưu", "tái cấu trúc"],
    ) {
        return "refactor".to_string();
    }

    if contains_any_word(
        &task_lower,
        &["build", "implement", "triển khai", "phát triển"],
    ) {
        return "build".to_string();
    }

    if !raw.is_empty() {
        raw
    } else {
        "plan".to_string()
    }
}

/// Detects whether a task qualifies for Minor Task Fast-Path Exemption.
pub fn is_minor_task(task: &str, arguments: &Value) -> bool {
    let norm_task = task.trim().to_lowercase().replace('-', "_");
    if contains_word(&norm_task, "generative_ui") {
        return false;
    }

    // 1. Explicit argument flag
    if arguments
        .get("is_minor")
        .or_else(|| arguments.get("fast_path"))
        .or_else(|| arguments.get("minor"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return true;
    }

    let t = task.trim().to_lowercase();
    if t.is_empty() {
        return false;
    }

    // 2. Negative guardrails: major/architectural changes can never be fast-path
    let negative_keywords = [
        "refactor", "refactoring", "redesign", "rewrite", "rewriting",
        "migrate", "migration", "migrating", "architecture", "architectural",
        "breaking change", "schema change", "database", "security audit",
        "performance overhaul", "re-architect",
    ];
    if contains_any_word(&t, &negative_keywords) {
        return false;
    }

    // 3. Positive keyword heuristics (multilingual EN + VI)
    let positive_keywords = [
        "typo", "spelling", "fix spelling", "fix comment", "update comment",
        "update docs", "update readme", "fix doc", "fix docs",
        "update documentation", "fix documentation", "edit documentation",
        "changelog", "rename variable", "fix grammar", "clean comment",
        "sửa lỗi chính tả", "cập nhật docs", "cập nhật tài liệu", "sửa comment",
        "sửa chú thích", "sửa readme", "sửa typo", "chỉnh tài liệu",
    ];
    contains_any_word(&t, &positive_keywords)
}

/// Generates the concise, actionable fast-path activation response.
pub fn build_fast_path_response(task: &str, proj_path: &Path) -> String {
    format!(
        "# Task Pipeline Activated [Minor Task Fast-Path Exemption]\n\n\
        - **Task**: {}\n\
        - **Workspace**: `{}`\n\
        - **Stage**: `Build` (plan_approved: true, is_fast_path: true)\n\n\
        ## Fast-Path Execution Protocol\n\
        1. **Ceremonies Bypassed**: Skill proposal modal (`ask_question`) and formal `implementation_plan.md` bypassed.\n\
        2. **Immediate Action**: You may inspect and authorize target file edits directly in 1 turn via `project_context(read, authorize=true)` or `workflow_gate(action=\"authorize_edit\")`.\n\
        3. **Boundary Guard**: Maximum 1 code file (or 3 doc/config files) permitted under Fast-Path; 300 LOC is a soft cohesion review limit.\n\
        4. **Verification**: Automated tests/verification required for code edits; pure markdown/doc edits exempt from `guidance(verify)`.\n",
        task,
        proj_path.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_detect_explicit_flag() {
        assert!(is_minor_task("complex feature", &json!({"is_minor": true})));
        assert!(is_minor_task("complex feature", &json!({"fast_path": true})));
        assert!(is_minor_task("complex feature", &json!({"minor": true})));
    }

    #[test]
    fn test_detect_positive_keywords() {
        assert!(is_minor_task("Fix typo in README.md", &json!({})));
        assert!(is_minor_task("Update docs for clustered read", &json!({})));
        assert!(is_minor_task("sửa lỗi chính tả trong file config", &json!({})));
        assert!(is_minor_task("rename variable foo to bar", &json!({})));
    }

    #[test]
    fn test_rejects_negative_keywords() {
        assert!(!is_minor_task("Fix typo and refactor database schema", &json!({})));
        assert!(!is_minor_task("Update docs and rewrite architecture", &json!({})));
    }
}
