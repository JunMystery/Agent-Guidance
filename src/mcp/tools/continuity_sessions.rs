use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use crate::mcp::state::ServerState;

type CoChangeSessionMap = Mutex<HashMap<String, HashSet<(String, String)>>>;

static RECORDED_CO_CHANGES: OnceLock<CoChangeSessionMap> = OnceLock::new();

fn co_change_session_key(proj_path: &Path, session_id: &str) -> String {
    let canon = proj_path
        .canonicalize()
        .unwrap_or_else(|_| proj_path.to_path_buf());
    format!("{}::{}", canon.to_string_lossy(), session_id)
}

pub(crate) fn handle_save(state: &ServerState, proj_path: &Path) -> String {
    // Milestone v1.9.0: Evolutionary Coupling Graph - record pairwise co-changes once per session
    if state.modified_files.len() >= 2 {
        if let Ok(db) = crate::context::db::CodeGraphDb::open_for_project(proj_path) {
            let mut unique_files: Vec<String> = state
                .modified_files
                .iter()
                .map(|f| crate::context::co_change::recorder::clean_path(f))
                .filter(|f| !f.is_empty())
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            unique_files.sort();

            let key = co_change_session_key(proj_path, &state.session_id);
            let map = RECORDED_CO_CHANGES.get_or_init(|| Mutex::new(HashMap::new()));
            let mut guard = map.lock().unwrap_or_else(|p| p.into_inner());
            let seen_pairs = guard.entry(key).or_default();

            for i in 0..unique_files.len() {
                for j in (i + 1)..unique_files.len() {
                    if let Some(pair) = crate::context::co_change::recorder::normalize_file_pair(
                        &unique_files[i],
                        &unique_files[j],
                    ) {
                        if !seen_pairs.contains(&pair)
                            && crate::context::co_change::recorder::record_co_change(
                                &db.conn,
                                &pair.0,
                                &pair.1,
                            )
                            .unwrap_or(false)
                        {
                            seen_pairs.insert(pair);
                        }
                    }
                }
            }
        }
    }

    match state.save_to_dir(proj_path) {
        Ok(_) => format!(
            "# Session Continuity\n\nSession state saved successfully to `.agent-context/sessions/{}.json`.",
            state.session_id
        ),
        Err(e) => format!("Failed to save session: {}", e),
    }
}

fn apply_zero_trust_reset(state: &mut ServerState, reset_session_scope: bool) {
    state.priority_gate_passed = false;
    state.plan_approved = false;
    state.edit_authorized = false;
    state.verification_passed = false;
    state.verification_command = None;
    state.expected_output_keyword = None;
    state.fix_attempts = 0;
    state.clear_fast_path();
    state.clear_deferred_authorized_paths();
    state.pending_skill_proposals.clear();
    if reset_session_scope {
        state.active_architecture_pattern = None;
    }
}

pub(crate) fn handle_load(state: &mut ServerState, proj_path: &Path) -> String {
    let result = if !state.session_id.is_empty() {
        ServerState::load_session_by_id(proj_path, &state.session_id)
            .or_else(|_| ServerState::load_from_dir(proj_path))
    } else {
        ServerState::load_from_dir(proj_path)
    };

    match result {
        Ok(mut loaded) => {
            let prev_cancel = state.cancellation.clone();
            let prev_roots = state.workspace_roots.clone();
            if loaded.workspace_roots.is_empty() {
                loaded.workspace_roots = prev_roots;
            }
            loaded.cancellation = prev_cancel;
            *state = loaded;
            apply_zero_trust_reset(state, true);
            format!(
                "# Session Continuity\n\nLoaded state for session '{}'. Permission flags reset (plan_approved=false). Total Calls: {}",
                state.session_id, state.tool_calls
            )
        }
        Err(e) => format!("Failed to load session: {}", e),
    }
}

pub(crate) fn handle_clear(
    arguments: &Value,
    state: &mut ServerState,
    proj_path: &Path,
) -> String {
    let clear_all = arguments
        .get("all")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let sessions_dir = proj_path.join(".agent-context").join("sessions");
    let legacy_file = proj_path.join(".agent-context").join("session.json");

    if clear_all {
        if sessions_dir.exists() {
            let _ = std::fs::remove_dir_all(&sessions_dir);
        }
        if legacy_file.exists() {
            let _ = std::fs::remove_file(&legacy_file);
        }
        let _ = crate::mcp::snapshots::clear_all_snapshots(proj_path);
        if let Some(map) = RECORDED_CO_CHANGES.get() {
            let prefix = format!(
                "{}::",
                proj_path
                    .canonicalize()
                    .unwrap_or_else(|_| proj_path.to_path_buf())
                    .to_string_lossy()
            );
            if let Ok(mut guard) = map.lock() {
                guard.retain(|k, _| !k.starts_with(&prefix));
            }
        }
    } else {
        if ServerState::is_valid_session_id(&state.session_id) {
            let active_session_file = sessions_dir.join(format!("{}.json", state.session_id));
            if active_session_file.exists() {
                let _ = std::fs::remove_file(&active_session_file);
            }
            let clean_snap = state.session_id.replace(['/', '\\', ':', '.'], "_");
            let active_snap_dir = proj_path
                .join(".agent-context")
                .join("snapshots")
                .join(clean_snap);
            if active_snap_dir.exists() {
                let _ = std::fs::remove_dir_all(&active_snap_dir);
            }
        }
        if legacy_file.exists() {
            let legacy_matches_active = std::fs::read_to_string(&legacy_file)
                .ok()
                .and_then(|c| serde_json::from_str::<ServerState>(&c).ok())
                .map(|s| s.session_id == state.session_id)
                .unwrap_or(false);
            let no_remaining_sessions = !sessions_dir.exists()
                || std::fs::read_dir(&sessions_dir)
                    .map(|mut d| {
                        !d.any(|e| {
                            e.ok()
                                .and_then(|ent| {
                                    ent.path()
                                        .extension()
                                        .and_then(|ext| ext.to_str())
                                        .map(|ext| ext == "json")
                                })
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(true);
            if legacy_matches_active || no_remaining_sessions {
                let _ = std::fs::remove_file(&legacy_file);
            }
        }
        if let Some(map) = RECORDED_CO_CHANGES.get() {
            let key = co_change_session_key(proj_path, &state.session_id);
            if let Ok(mut guard) = map.lock() {
                guard.remove(&key);
            }
        }
    }

    let prev_cancel = state.cancellation.clone();
    let prev_roots = state.workspace_roots.clone();
    let prev_client = state.agent_client_name.clone();
    let mut fresh = ServerState::with_client_name(prev_client);
    fresh.workspace_roots = prev_roots;
    fresh.cancellation = prev_cancel;
    *state = fresh;

    if clear_all {
        "# Session Continuity: [clear]\n\nAll saved sessions and rollback snapshots cleared successfully. Active session state reset.".to_string()
    } else {
        "# Session Continuity: [clear]\n\nActive session state and rollback snapshots cleared successfully.".to_string()
    }
}

pub(crate) fn handle_list(state: &ServerState, proj_path: &Path) -> String {
    let sessions = ServerState::list_sessions(proj_path);
    if sessions.is_empty() {
        "# Active & Archived Sessions\n\nNo session snapshots found in `.agent-context/sessions/`.".to_string()
    } else {
        let mut table = String::from("# Active & Archived Sessions\n\n| Session ID | Client / IDE | Workflow Stage | Architecture | Modified Files |\n| :--- | :--- | :--- | :--- | :---: |\n");
        for s in &sessions {
            let is_current = s.session_id == state.session_id;
            let id_str = if is_current {
                format!("`{}` **(Current)**", s.session_id)
            } else {
                format!("`{}`", s.session_id)
            };
            let client_str = s.agent_client_name.as_deref().unwrap_or("Default");
            let arch_str = s.active_architecture_pattern.as_deref().unwrap_or("Auto");
            let mod_count = format!("{} files", s.modified_files.len());
            table.push_str(&format!(
                "| {} | {} | `{}` | {} | {} |\n",
                id_str, client_str, s.workflow_stage, arch_str, mod_count
            ));
        }
        table.push_str("\n**To switch**: Run `session_continuity(operation=\"switch\", session_id=\"<session_id>\")`");
        table
    }
}

pub(crate) fn handle_switch(
    arguments: &Value,
    state: &mut ServerState,
    proj_path: &Path,
) -> String {
    let target_id = arguments
        .get("session_id")
        .or_else(|| arguments.get("conversation_id"))
        .or_else(|| arguments.get("chat_id"))
        .or_else(|| arguments.get("id"))
        .and_then(|s| s.as_str())
        .unwrap_or("");

    if target_id.trim().is_empty() {
        "# Session Continuity: [switch]\n\nError: `session_id` parameter is required for switch operation. Call `session_continuity(operation=\"list\")` to view available sessions.".to_string()
    } else {
        match ServerState::load_session_by_id(proj_path, target_id.trim()) {
            Ok(mut loaded) => {
                apply_zero_trust_reset(&mut loaded, false);
                let prev_id = state.session_id.clone();
                let prev_cancel = state.cancellation.clone();
                let prev_roots = state.workspace_roots.clone();
                if loaded.workspace_roots.is_empty() {
                    loaded.workspace_roots = prev_roots;
                }
                loaded.cancellation = prev_cancel;
                *state = loaded;

                format!(
                    "# Session Continuity: [switch]\n\nSuccessfully switched active session from `{}` to `{}`.\n\n- Active Workflow Stage: `{}`\n- Client / IDE: `{}`\n- Architecture Pattern: `{}`\n- Modified Files: {}\n- Zero-Trust Security: `plan_approved=false`, `edit_authorized=false` (Call `task_pipeline` or `workflow_gate` to proceed).",
                    prev_id,
                    state.session_id,
                    state.workflow_stage,
                    state.agent_client_name.as_deref().unwrap_or("Default"),
                    state.active_architecture_pattern.as_deref().unwrap_or("Auto"),
                    state.modified_files.len()
                )
            }
            Err(err) => format!("# Session Continuity: [switch]\n\nFailed to switch session: {}", err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    static TEST_SEQ: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn test_clear_removes_legacy_session_and_preserves_runtime_context() {
        let temp_dir = std::env::temp_dir().join(format!(
            "ag_clear_legacy_{}_{}",
            std::process::id(),
            TEST_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let flag = Arc::new(AtomicBool::new(false));
        let mut state = ServerState::with_client_name(Some("antigravity".to_string()));
        state.workspace_roots = vec![temp_dir.to_string_lossy().to_string()];
        state.set_cancellation(flag.clone());
        state.tool_calls = 42;
        state.save_to_dir(&temp_dir).unwrap();

        let legacy_file = temp_dir.join(".agent-context").join("session.json");
        assert!(legacy_file.exists());

        handle_clear(&serde_json::json!({}), &mut state, &temp_dir);
        assert!(!legacy_file.exists(), "Legacy session.json must be deleted on clear");
        assert_eq!(state.workspace_roots, vec![temp_dir.to_string_lossy().to_string()]);
        assert!(state.cancellation.is_some());

        // Subsequent load must not resurrect the cleared session
        handle_load(&mut state, &temp_dir);
        assert_eq!(state.tool_calls, 0);
        assert!(state.cancellation.is_some());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
