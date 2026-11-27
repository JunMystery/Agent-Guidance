use serde_json::Value;
use std::path::Path;

use crate::mcp::state::ServerState;

pub(crate) fn handle_save(state: &ServerState, proj_path: &Path) -> String {
    // Milestone v1.9.0: Evolutionary Coupling Graph - record pairwise co-changes
    if state.modified_files.len() >= 2 {
        if let Ok(db) = crate::context::db::CodeGraphDb::open_for_project(proj_path) {
            let _ = crate::context::co_change::record_session_co_changes(&db.conn, &state.modified_files);
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
        state.priority_gate_passed = false;
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

pub(crate) fn handle_clear(state: &mut ServerState, proj_path: &Path) -> String {
    let dir = proj_path.join(".agent-context").join("sessions");
    if dir.exists() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    let legacy_file = proj_path.join(".agent-context").join("session.json");
    if legacy_file.exists() {
        let _ = std::fs::remove_file(&legacy_file);
    }
    let _ = crate::mcp::snapshots::clear_all_snapshots(proj_path);
    let prev_cancel = state.cancellation.clone();
    let prev_roots = state.workspace_roots.clone();
    let prev_client = state.agent_client_name.clone();
    let mut fresh = ServerState::with_client_name(prev_client);
    fresh.workspace_roots = prev_roots;
    fresh.cancellation = prev_cancel;
    *state = fresh;
    "# Session Continuity: [clear]\n\nSession state and rollback snapshots cleared successfully. Active session state reset.".to_string()
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

        handle_clear(&mut state, &temp_dir);
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
