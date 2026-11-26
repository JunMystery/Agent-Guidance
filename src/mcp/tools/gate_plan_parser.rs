use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

use crate::mcp::state::ServerState;

const KNOWN_EXTS: &[&str] = &[
    "rs", "js", "ts", "py", "json", "toml", "md", "html", "css", "go", "java", "c", "cpp", "h",
    "sql", "yaml", "yml", "sh",
];

pub(crate) fn find_plan_artifact(
    proj_path: &Path,
    custom_path: Option<&str>,
    session_id: Option<&str>,
) -> Option<PathBuf> {
    if let Some(custom) = custom_path {
        let p = PathBuf::from(custom);
        if p.is_absolute() && p.exists() {
            return Some(p);
        }
        let rel = proj_path.join(custom);
        if rel.exists() {
            return Some(rel);
        }
    }

    if let Some(sid) = session_id.filter(|s| !s.is_empty() && !s.starts_with("session_")) {
        if let Some(brain_plan) = find_in_brain_dir(Some(sid)) {
            return Some(brain_plan);
        }
    }

    for cand in &[proj_path.join("implementation_plan.md"), proj_path.join(".agent-context/implementation_plan.md")] {
        if cand.exists() {
            return Some(cand.clone());
        }
    }

    find_in_brain_dir(session_id)
}

fn find_in_brain_dir(session_id: Option<&str>) -> Option<PathBuf> {
    let brain = match std::env::var("AGY_BRAIN_DIR") {
        Ok(custom) if !custom.trim().is_empty() => PathBuf::from(custom),
        _ => dirs::home_dir()?.join(".gemini").join("antigravity").join("brain"),
    };
    if !brain.is_dir() {
        return None;
    }
    if let Some(sid) = session_id {
        let plan = brain.join(sid).join("implementation_plan.md");
        if plan.exists() {
            return Some(plan);
        }
    }
    if let Ok(cid) = std::env::var("AGY_CONVERSATION_ID").or_else(|_| std::env::var("AGY_SESSION_ID")) {
        let plan = brain.join(cid).join("implementation_plan.md");
        if plan.exists() {
            return Some(plan);
        }
    }
    None
}

pub(crate) fn extract_paths_from_plan_content(content: &str, proj_path: &Path) -> Vec<String> {
    let mut results = Vec::new();
    let mut in_target = false;

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.starts_with('#') {
            let lower = line.to_lowercase();
            in_target = lower.contains("proposed changes")
                || lower.contains("target file")
                || lower.contains("files to modify")
                || lower.contains("user review required")
                || lower.contains("architecture design")
                || lower.contains("implementation plan");
        }
        extract_candidates_from_line(line, proj_path, &mut results, in_target);
    }
    results
}

fn extract_candidates_from_line(line: &str, proj_path: &Path, res: &mut Vec<String>, in_target: bool) {
    if !in_target && !line.starts_with('|') && !line.starts_with('-') && !line.starts_with('*') {
        return;
    }
    for part in line.split('`') {
        sanitize_and_push(part, proj_path, res);
    }
    let mut start = 0;
    while let Some(open_p) = line[start..].find("](") {
        let idx = start + open_p + 2;
        if let Some(close_p) = line[idx..].find(')') {
            sanitize_and_push(&line[idx..idx + close_p], proj_path, res);
            start = idx + close_p + 1;
        } else {
            break;
        }
    }
}

fn sanitize_and_push(raw: &str, proj_path: &Path, results: &mut Vec<String>) {
    let mut cleaned = raw.trim()
        .trim_start_matches("file://")
        .trim_matches(|c| matches!(c, '\'' | '"' | '(' | ')' | '[' | ']'));
    if let Some(pos) = cleaned.find(['#', '?']) {
        cleaned = &cleaned[..pos];
    }
    if cleaned.starts_with('/') && cleaned.as_bytes().get(2) == Some(&b':') {
        cleaned = cleaned.trim_start_matches('/');
    }
    if cleaned.is_empty() || cleaned.contains(' ') || cleaned.contains('\n') || cleaned.contains('*') {
        return;
    }
    let norm = cleaned.replace('\\', "/");
    let proj_norm = proj_path.to_string_lossy().replace('\\', "/");
    let candidate = if let Some(s) = norm.strip_prefix(&proj_norm) {
        s.trim_start_matches('/').to_string()
    } else if norm.len() >= proj_norm.len() && norm[..proj_norm.len()].eq_ignore_ascii_case(&proj_norm) {
        norm[proj_norm.len()..].trim_start_matches('/').to_string()
    } else {
        norm
    };
    if candidate == "implementation_plan.md" || candidate.starts_with("http://") || candidate.starts_with("https://") {
        return;
    }
    let ext = match Path::new(&candidate).extension().and_then(|e| e.to_str()) {
        Some(e) => e.to_lowercase(),
        None => return,
    };
    if KNOWN_EXTS.contains(&ext.as_str()) && !results.contains(&candidate) {
        results.push(candidate);
    }
}

pub(crate) fn auto_discover_plan_paths(
    proj_path: &Path,
    arguments: &Value,
    state: &ServerState,
) -> (Vec<String>, Option<PathBuf>) {
    let custom = arguments.get("artifact_path").or_else(|| arguments.get("plan_path")).and_then(|v| v.as_str());
    let found = find_plan_artifact(proj_path, custom, Some(&state.session_id));
    if let Some(content) = found.as_ref().and_then(|p| fs::read_to_string(p).ok()) {
        return (extract_paths_from_plan_content(&content, proj_path), found);
    }
    (Vec::new(), None)
}
