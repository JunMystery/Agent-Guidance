use anyhow::Result;
use std::fs;
use std::path::{Component, Path, PathBuf};
use tracing::info;

use crate::mcp::state::ServerState;
use super::{LearningItem, parse_learnings_file, handoff_file_path, write_file_atomically};

/// Matches category keywords when ML embeddings are offline.
pub fn match_category_keywords(items: &[LearningItem], task: &str, limit: usize) -> Vec<String> {
    let task_lower = task.to_lowercase();
    let mut keyword_matched: Vec<&LearningItem> = Vec::new();

    let target_categories: Vec<&str> = if task_lower.contains("test")
        || task_lower.contains("cargo test")
        || task_lower.contains("bench")
        || task_lower.contains("mock")
    {
        vec!["build_test"]
    } else if task_lower.contains("arch")
        || task_lower.contains("pattern")
        || task_lower.contains("layer")
        || task_lower.contains("module")
        || task_lower.contains("structure")
    {
        vec!["arch"]
    } else if task_lower.contains("error")
        || task_lower.contains("bug")
        || task_lower.contains("fail")
        || task_lower.contains("panic")
        || task_lower.contains("fix")
    {
        vec!["gotcha"]
    } else if task_lower.contains("env")
        || task_lower.contains("path")
        || task_lower.contains("config")
        || task_lower.contains("setup")
    {
        vec!["env"]
    } else if task_lower.contains("domain")
        || task_lower.contains("logic")
        || task_lower.contains("business")
    {
        vec!["domain"]
    } else {
        vec![]
    };

    if !target_categories.is_empty() {
        for item in items.iter().rev() {
            if target_categories.contains(&item.category.as_str()) {
                keyword_matched.push(item);
                if keyword_matched.len() >= limit {
                    break;
                }
            }
        }
    }

    keyword_matched
        .into_iter()
        .map(|i| {
            let cat_label = if i.is_pinned {
                format!("PINNED:{}", i.category)
            } else {
                i.category.clone()
            };
            format!("- [{}] {}", cat_label, i.content)
        })
        .collect()
}

/// Retrieves the top `limit` relevant project learnings using hybrid scoring (semantic vector + recency),
/// falling back to category keyword matching when offline, or returning empty in Strict Context mode.
pub fn get_semantic_relevant_learnings(
    proj_path: &Path,
    task: &str,
    limit: usize,
    threshold: f32,
) -> Vec<String> {
    let items = parse_learnings_file(proj_path);
    if items.is_empty() || task.trim().is_empty() {
        return Vec::new();
    }

    let total = items.len();

    // Tier 1: Try ML Embeddings Vector Search (Multilingual-E5 / ONNX) via Fast Single Batch
    if let Some(model) = crate::ml::embeddings::try_cached_model() {
        if let Ok(task_vec) = model.embed_text(task, Some("query")) {
            let texts: Vec<String> = items
                .iter()
                .map(|item| format!("{}: {}", item.category, item.content))
                .collect();
            let text_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();

            if let Ok(item_vecs) = model.embed_batch(&text_refs, Some("passage"), 32) {
                let mut scored: Vec<(f32, &LearningItem)> = Vec::with_capacity(items.len());

                for (idx, (item, item_vec)) in items.iter().zip(item_vecs.iter()).enumerate() {
                    let sim = crate::ml::embeddings::cosine_similarity(&task_vec, item_vec);
                    let recency_weight = (idx + 1) as f32 / total as f32;
                    let hybrid_score = 0.8 * sim + 0.2 * recency_weight;

                    if sim >= threshold {
                        scored.push((hybrid_score, item));
                    }
                }

                if !scored.is_empty() {
                    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
                    return scored
                        .into_iter()
                        .take(limit)
                        .map(|(_, i)| {
                            let cat_label = if i.is_pinned {
                                format!("PINNED:{}", i.category)
                            } else {
                                i.category.clone()
                            };
                            format!("- [{}] {}", cat_label, i.content)
                        })
                        .collect();
                } else {
                    // Strict Context: no items met threshold
                    return Vec::new();
                }
            }
        }
    }

    // Tier 2: Category Keyword Matching Fallback (when ML offline / weights unavailable)
    let keyword_results = match_category_keywords(&items, task, limit);
    if !keyword_results.is_empty() {
        return keyword_results;
    }

    // Tier 3: Strict Context empty return
    Vec::new()
}

/// Computes simple line delta (additions, deletions) between two texts.
pub fn compute_line_delta(original: &str, current: &str) -> (usize, usize) {
    use std::collections::HashSet;
    let orig_set: HashSet<&str> = original.lines().collect();
    let curr_set: HashSet<&str> = current.lines().collect();

    let additions = current.lines().filter(|l| !orig_set.contains(l)).count();
    let deletions = original.lines().filter(|l| !curr_set.contains(l)).count();
    (additions, deletions)
}

fn resolve_snapshot_file(proj_path: &Path, snapshots_dir: &Path, file: &str) -> Option<PathBuf> {
    let trimmed = file.trim();
    if trimmed.is_empty() || trimmed.contains("..") {
        return None;
    }
    let candidate = Path::new(trimmed);
    let rel = if candidate.is_absolute() {
        let can_proj = proj_path.canonicalize().ok();
        candidate
            .strip_prefix(proj_path)
            .ok()
            .or_else(|| can_proj.as_ref().and_then(|cp| candidate.strip_prefix(cp).ok()))
            .map(Path::to_path_buf)
            .or_else(|| {
                let can_cand = candidate.canonicalize().ok()?;
                can_cand
                    .strip_prefix(can_proj.as_ref()?)
                    .ok()
                    .map(Path::to_path_buf)
            })?
    } else {
        candidate.to_path_buf()
    };

    if rel.as_os_str().is_empty()
        || !rel.components().all(|c| matches!(c, Component::Normal(_)))
    {
        return None;
    }

    let snap_file = snapshots_dir.join(&rel);
    if snap_file.is_file() {
        Some(snap_file)
    } else {
        None
    }
}

/// Generates a comprehensive markdown modification summary for the active session.
pub fn generate_session_diff_summary(proj_path: &Path, state: &ServerState) -> String {
    if state.modified_files.is_empty() {
        return format!(
            "# Session Modification Summary\n\n- Active Session ID: `{}`\n- Total Files Modified: 0\n\n*No files have been modified in this session yet.*",
            state.session_id
        );
    }

    let mut table = String::from("| File Path | Impact Risk | Snapshot Available | Lines Delta |\n| :--- | :---: | :---: | :---: |\n");
    let snapshots_dir = crate::mcp::snapshots::get_session_snapshot_dir(proj_path, &state.session_id);

    for file in &state.modified_files {
        let impact = crate::mcp::impact::assess_file_risk(proj_path, file);
        let snap_file = resolve_snapshot_file(proj_path, &snapshots_dir, file);
        let snap_str = if snap_file.is_some() { "Yes" } else { "No" };

        let curr_file_path = proj_path.join(file);
        let curr_content = fs::read_to_string(&curr_file_path).unwrap_or_default();

        let delta_str = if let Some(ref snap_path) = snap_file {
            let orig_content = fs::read_to_string(snap_path).unwrap_or_default();
            let (adds, dels) = compute_line_delta(&orig_content, &curr_content);
            format!("+{} / -{}", adds, dels)
        } else {
            let lines = curr_content.lines().count();
            format!("+{} / -0", lines)
        };

        table.push_str(&format!(
            "| `{}` | {:?} | {} | {} |\n",
            file, impact.risk_level, snap_str, delta_str
        ));
    }

    format!(
        "# Session Modification Summary\n\n- Active Session ID: `{}`\n- Total Files Modified: {}\n\n{}\n**Handoff Command**: Call `session_continuity(operation=\"handoff\", next_action=\"...\")` to conclude session.",
        state.session_id,
        state.modified_files.len(),
        table
    )
}

/// Writes and returns a concise Cross-Agent Handoff Summary document.
pub fn write_handoff_summary(proj_path: &Path, state: &ServerState, next_action: &str) -> Result<String> {
    let handoff_path = handoff_file_path(proj_path);

    let next_step = if next_action.trim().is_empty() {
        "Inspect recent changes via `project_context(operation=\"read\")` and continue task execution."
    } else {
        next_action.trim()
    };

    let modified_section = if state.modified_files.is_empty() {
        "None (No files modified during this session).".to_string()
    } else {
        let mut table = String::from("| File Path | Impact Risk | Snapshot Available |\n| :--- | :---: | :---: |\n");
        let snapshots_dir = crate::mcp::snapshots::get_session_snapshot_dir(proj_path, &state.session_id);
        for file in &state.modified_files {
            let impact = crate::mcp::impact::assess_file_risk(proj_path, file);
            let has_snap = if resolve_snapshot_file(proj_path, &snapshots_dir, file).is_some() {
                "Yes"
            } else {
                "No"
            };
            table.push_str(&format!("| `{}` | {:?} | {} |\n", file, impact.risk_level, has_snap));
        }
        table
    };

    let summary = format!(
        "# Cross-Agent Handoff Protocol\n\n- Active Session ID: `{}`\n- Workflow Stage: `{}` (Plan Approved: {})\n- Active Architecture Pattern: `{}`\n- Total Tool Calls: {}\n\n## Modified Files in This Session:\n{}\n\n## Target Goal / Intent:\n{}\n\n## Next Action for Incoming Agent:\n{}\n\n---\n*Written to `.agent-context/handoff.md` for zero-delay multi-IDE handoff.*",
        state.session_id,
        state.workflow_stage,
        state.plan_approved,
        state.active_architecture_pattern.as_deref().unwrap_or("Auto"),
        state.tool_calls,
        modified_section,
        state.user_intent_summary.as_deref().unwrap_or("Active development task in progress."),
        next_step
    );

    write_file_atomically(&handoff_path, &summary)?;
    info!("Saved cross-agent handoff summary to `.agent-context/handoff.md`");
    Ok(summary)
}

#[cfg(test)]
mod matching_tests {
    use super::*;

    #[test]
    fn test_session_diff_and_handoff_resolve_real_snapshots() {
        let temp_dir = std::env::temp_dir().join(format!(
            "ag_matching_snap_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&temp_dir);
        let src_dir = temp_dir.join("src");
        fs::create_dir_all(&src_dir).unwrap();

        let rel_path = "src/example.rs";
        let file_path = temp_dir.join(rel_path);
        fs::write(&file_path, "line 1\nline 2\nline 3\n").unwrap();

        let mut state = ServerState::new();
        state.session_id = "sess.pass7:test".to_string();
        let created =
            crate::mcp::snapshots::create_file_snapshot(&temp_dir, rel_path, &state.session_id)
                .unwrap();
        assert!(created);

        fs::write(&file_path, "line 1\nline 2 edited\nline 3\nline 4\n").unwrap();
        state.record_modified_file(rel_path);
        state.record_modified_file("../escape.rs");

        let diff_summary = generate_session_diff_summary(&temp_dir, &state);
        assert!(
            diff_summary.contains("| `src/example.rs` |") && diff_summary.contains("| Yes | +2 / -1 |"),
            "Expected snapshot Yes and +2 / -1 delta, got:\n{}",
            diff_summary
        );
        assert!(
            diff_summary.contains("| `../escape.rs` |") && diff_summary.contains("| No |"),
            "Expected traversal path to report No snapshot, got:\n{}",
            diff_summary
        );

        let handoff = write_handoff_summary(&temp_dir, &state, "Continue Pass 7").unwrap();
        assert!(
            handoff.contains("| `src/example.rs` |") && handoff.contains("| Yes |"),
            "Expected handoff summary to find snapshot, got:\n{}",
            handoff
        );
        assert!(handoff_file_path(&temp_dir).is_file());

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_parse_learnings_non_ascii_header_no_panic() {
        let temp_dir = std::env::temp_dir().join(format!(
            "ag_learnings_utf8_header_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&temp_dir);
        let learnings_path = super::super::learnings_file_path(&temp_dir);
        let content = "## [pinned:arch]\n- Keep modules cohesive\n## [\u{00df}123456]\n- Safe non-ascii header\n## [a\u{00e9}\u{00e9}\u{00e9}\u{00e9}]\n- Multi-byte boundary test\n";
        write_file_atomically(&learnings_path, content).unwrap();

        let items = parse_learnings_file(&temp_dir);
        assert_eq!(items.len(), 3);
        assert!(items[0].is_pinned);
        assert_eq!(items[0].category, "arch");
        assert!(!items[1].is_pinned);
        assert_eq!(items[1].category, "\u{00df}123456");
        assert!(!items[2].is_pinned);

        let _ = fs::remove_dir_all(&temp_dir);
    }
}

