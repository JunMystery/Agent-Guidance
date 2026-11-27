use serde_json::Value;
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::mcp::state::ServerState;
use crate::mcp::state::uri::parse_file_uri;
use crate::mcp::state::workspace::strip_verbatim_prefix;

const KNOWN_EXTS: &[&str] = &[
    "rs", "js", "ts", "tsx", "jsx", "vue", "svelte", "py", "json", "toml", "md", "html", "css",
    "scss", "xml", "proto", "go", "java", "kt", "swift", "cs", "rb", "php", "c", "cpp", "h",
    "sql", "yaml", "yml", "sh", "ps1",
];

fn is_markdown_plan_path(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| matches!(e.to_ascii_lowercase().as_str(), "md" | "markdown"))
        .unwrap_or(false)
}

fn resolve_brain_dir() -> Option<PathBuf> {
    match std::env::var("AGY_BRAIN_DIR") {
        Ok(custom) if !custom.trim().is_empty() => Some(PathBuf::from(custom)),
        _ => Some(dirs::home_dir()?.join(".gemini").join("antigravity").join("brain")),
    }
}

fn has_parent_dir_segment(s: &str) -> bool {
    Path::new(s)
        .components()
        .any(|c| matches!(c, Component::ParentDir))
        || s.replace('\\', "/").split('/').any(|seg| seg == "..")
}

pub(crate) fn find_plan_artifact(
    proj_path: &Path,
    custom_path: Option<&str>,
    session_id: Option<&str>,
) -> Option<PathBuf> {
    if let Some(custom) = custom_path
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.contains('\0') && !has_parent_dir_segment(s))
    {
        let p = PathBuf::from(custom);
        if is_markdown_plan_path(&p) {
            if p.is_absolute() {
                if let Ok(can_p) = p.canonicalize() {
                    let in_proj = proj_path
                        .canonicalize()
                        .map(|cp| can_p.starts_with(&cp))
                        .unwrap_or(false);
                    let in_brain = resolve_brain_dir()
                        .and_then(|b| b.canonicalize().ok())
                        .map(|cb| can_p.starts_with(&cb))
                        .unwrap_or(false);
                    if (in_proj || in_brain) && can_p.exists() {
                        return Some(strip_verbatim_prefix(&can_p));
                    }
                }
            } else if let Ok(rel) = super::helpers::validate_path(proj_path, custom) {
                if rel.exists() {
                    return Some(strip_verbatim_prefix(&rel));
                }
            }
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
    let brain = resolve_brain_dir()?;
    if !brain.is_dir() {
        return None;
    }
    if let Some(sid) = session_id {
        let trimmed = sid.trim();
        if ServerState::is_valid_session_id(trimmed) {
            let plan = brain.join(trimmed).join("implementation_plan.md");
            if plan.exists() {
                return Some(plan);
            }
        }
    }
    if let Ok(cid) = std::env::var("AGY_CONVERSATION_ID").or_else(|_| std::env::var("AGY_SESSION_ID")) {
        let trimmed = cid.trim();
        if ServerState::is_valid_session_id(trimmed) {
            let plan = brain.join(trimmed).join("implementation_plan.md");
            if plan.exists() {
                return Some(plan);
            }
        }
    }
    None
}

fn parse_heading(line: &str) -> Option<(usize, String)> {
    let level = line.bytes().take_while(|&b| b == b'#').count();
    if level == 0 {
        return None;
    }
    let rest = &line[level..];
    if !rest.is_empty() && !rest.starts_with(' ') && !rest.starts_with('\t') {
        return None;
    }
    Some((level, rest.trim().to_lowercase()))
}

fn is_excluded_heading(lower: &str) -> bool {
    lower.contains("out of scope")
        || lower.contains("non-goal")
        || lower.contains("verification")
        || lower.contains("test plan")
        || lower.contains("excluded")
        || lower.contains("rollback")
}

fn is_target_heading(lower: &str) -> bool {
    lower.contains("proposed changes")
        || lower.contains("target file")
        || lower.contains("files to modify")
        || lower.contains("files to change")
        || lower.contains("files to create")
        || lower.contains("user review required")
        || lower.contains("architecture design")
        || lower.contains("implementation plan")
}

pub(crate) fn extract_paths_from_plan_content(content: &str, proj_path: &Path) -> Vec<String> {
    let mut results = Vec::new();
    let mut in_target = false;
    let mut target_level: Option<usize> = None;
    let mut excluded_level: Option<usize> = None;
    let mut saw_heading = false;
    let mut fence_marker: Option<&str> = None;

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if let Some(marker) = fence_marker {
            if line.starts_with(marker) {
                fence_marker = None;
            }
            continue;
        }
        if line.starts_with("```") {
            fence_marker = Some("```");
            continue;
        }
        if line.starts_with("~~~") {
            fence_marker = Some("~~~");
            continue;
        }

        if let Some((level, lower)) = parse_heading(line) {
            saw_heading = true;
            if is_excluded_heading(&lower) {
                in_target = false;
                target_level = None;
                excluded_level = Some(level);
            } else if excluded_level.is_some_and(|ex| level > ex) {
                in_target = false;
            } else if is_target_heading(&lower) {
                in_target = true;
                target_level = Some(level);
                excluded_level = None;
            } else if target_level.is_some_and(|tl| level > tl) && in_target {
                excluded_level = None;
            } else {
                in_target = false;
                target_level = None;
                excluded_level = None;
            }
            continue;
        }

        let allow_line = in_target
            || (!saw_heading
                && (line.starts_with('|') || line.starts_with('-') || line.starts_with('*')));
        if allow_line {
            extract_candidates_from_line(line, proj_path, &mut results, in_target);
        }
    }
    results
}

fn extract_candidates_from_line(line: &str, proj_path: &Path, res: &mut Vec<String>, in_target: bool) {
    let parts: Vec<&str> = line.split('`').collect();
    for (idx, part) in parts.iter().enumerate() {
        if idx % 2 == 1 {
            // Only paired backtick spans (or last odd if line has odd number of backticks)
            if idx + 1 < parts.len() {
                sanitize_and_push(part, proj_path, res);
            }
        } else if in_target {
            // Also extract plain path tokens outside backticks under target sections
            for raw_tok in part.split_whitespace() {
                if raw_tok.contains("](") || raw_tok.contains("://") {
                    continue;
                }
                let trimmed = raw_tok.trim_matches(|c: char| {
                    matches!(c, '\'' | '"' | '(' | ')' | '[' | ']' | ',' | ';' | ':' | '|')
                });
                if trimmed.contains('/') || trimmed.contains('\\') {
                    sanitize_and_push(trimmed, proj_path, res);
                }
            }
        }
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
    let mut cleaned = raw
        .trim()
        .trim_matches(|c| matches!(c, '\'' | '"' | '(' | ')' | '[' | ']'))
        .to_string();
    if cleaned.is_empty() || cleaned.contains('\0') {
        return;
    }

    if let Some(pos) = cleaned.find(['#', '?']) {
        cleaned.truncate(pos);
    }

    if cleaned
        .as_bytes()
        .get(..7)
        .is_some_and(|b| b.eq_ignore_ascii_case(b"file://"))
    {
        cleaned = parse_file_uri(&cleaned);
    }

    let mut stripped = cleaned
        .trim()
        .trim_start_matches(r"\\?\")
        .trim_start_matches("//?/");

    let bytes = stripped.as_bytes();
    if bytes.len() >= 3
        && bytes[0] == b'/'
        && bytes[1].is_ascii_alphabetic()
        && bytes[2] == b':'
    {
        stripped = &stripped[1..];
    }

    if stripped.is_empty()
        || stripped.contains(' ')
        || stripped.contains('\n')
        || stripped.contains('*')
        || stripped.contains('\0')
    {
        return;
    }

    let norm = stripped.replace('\\', "/");
    if norm.starts_with("http://") || norm.starts_with("https://") || norm.contains("://") {
        return;
    }

    let proj_raw = proj_path.to_string_lossy();
    let proj_norm = proj_raw
        .trim_start_matches(r"\\?\")
        .trim_start_matches("//?/")
        .replace('\\', "/");
    let proj_trimmed = proj_norm.trim_end_matches('/');

    let candidate_raw = if !proj_trimmed.is_empty()
        && norm
            .get(..proj_trimmed.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(proj_trimmed))
    {
        let rest = &norm[proj_trimmed.len()..];
        if rest.is_empty() {
            String::new()
        } else if let Some(after_slash) = rest.strip_prefix('/') {
            after_slash.to_string()
        } else {
            norm
        }
    } else {
        norm
    };

    let candidate = candidate_raw.trim_start_matches("./").to_string();
    if candidate.is_empty()
        || candidate == "implementation_plan.md"
        || has_parent_dir_segment(&candidate)
    {
        return;
    }

    let ext = match Path::new(&candidate).extension().and_then(|e| e.to_str()) {
        Some(e) => e.to_lowercase(),
        None => return,
    };
    if !KNOWN_EXTS.contains(&ext.as_str()) {
        return;
    }

    // Avoid auto-authorizing prose tokens like `next.js` / `vue.js` / `node.js`
    // as [NEW FILE] unless the path has a directory separator or exists on disk.
    if !candidate.contains('/') && !proj_path.join(&candidate).exists() {
        return;
    }

    if !results.contains(&candidate) {
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
