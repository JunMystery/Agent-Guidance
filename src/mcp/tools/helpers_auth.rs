use serde_json::Value;
use std::path::Path;

use crate::mcp::state::ServerState;

pub(crate) fn evaluate_read_authorization(
    arguments: &Value,
    proj_path: &Path,
    paths: &[String],
    state: &mut ServerState,
) -> String {
    let authorize_requested = arguments
        .get("authorize")
        .or_else(|| arguments.get("pre_authorize"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let op = arguments.get("operation").and_then(|v| v.as_str()).unwrap_or("");
    let is_auth_op = op == "read_and_authorize" || op == "read_auth";

    if !authorize_requested && !is_auth_op {
        if !paths.is_empty() && !state.edit_authorized && !state.auth_tip_shown {
            state.auth_tip_shown = true;
            let gate_paths_json = serde_json::to_string(paths).unwrap_or_else(|_| "[]".to_string());
            return format!(
                "\n---\n> [!TIP] **One-Turn Edit Authorization**: `workflow_gate(action=\"authorize_edit\", project_path=\"{}\", relative_paths={}, risk_level=\"LOW\", justification=\"...\", architecture_pattern=\"Auto\")`",
                proj_path.display(),
                gate_paths_json
            );
        }
        return String::new();
    }

    if paths.is_empty() {
        return String::new();
    }

    let justification = arguments
        .get("justification")
        .and_then(|j| j.as_str())
        .unwrap_or("Pre-authorized via read_and_authorize");
    let allow_cross = arguments
        .get("allow_cross_workspace")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if state.plan_approved {
        if state.workflow_stage == "Plan" {
            let _ = state.set_stage("Build");
        }
        if !matches!(state.workflow_stage.as_str(), "Build" | "Fix") {
            return format!(
                "\n---\n# Edit Approval Gate Authorization\n\n- Status: BLOCKED\n- Active Stage: {}\n\nError: WORKFLOW_STAGE_BLOCKED: Edits require `Build` or `Fix` stage (current stage: `{}`). Advance to `Build` or `Fix` before authorizing edits.",
                state.workflow_stage, state.workflow_stage
            );
        }
        let raw_arch = arguments
            .get("architecture_pattern")
            .and_then(|a| a.as_str())
            .unwrap_or("Auto");
        let arch_pattern = super::helpers::resolve_architecture_pattern(raw_arch, proj_path, state);
        if !super::helpers::is_valid_architecture_pattern(&arch_pattern) {
            return format!(
                "\n---\n# Edit Approval Gate Authorization\n\n- Status: BLOCKED (ARCHITECTURE_GATE_BLOCKED)\n- Declared Architecture: '{}'\n\nError: ARCHITECTURE_GATE_BLOCKED: Select a valid `architecture_pattern` before authorizing edits.",
                if arch_pattern.is_empty() { "NONE" } else { &arch_pattern }
            );
        }
        if let Some(blocked) = super::gate_edit::enforce_edit_boundaries(proj_path, paths, allow_cross, state) {
            return format!("\n---\n{}", blocked);
        }
        let auth_report = super::gate_edit_batch::handle_batch_authorize(
            proj_path, paths, justification, &arch_pattern, allow_cross, state,
        );
        format!("\n---\n## Direct Edit Authorization Report\n\n{}", auth_report)
    } else {
        let mut normalized_paths = Vec::with_capacity(paths.len());
        for p in paths {
            match super::helpers::validate_path_scoped_normalized(proj_path, p, allow_cross) {
                Ok((_, norm)) if !norm.is_empty() => {
                    if !normalized_paths.contains(&norm) {
                        normalized_paths.push(norm);
                    }
                }
                Ok(_) => {}
                Err(err) => {
                    return format!(
                        "\n---\n# Edit Approval Gate Authorization\n\n- Status: BLOCKED (PATH_TRAVERSAL_PROHIBITED)\n- Target File: `{}`\n\nError: PATH_TRAVERSAL_PROHIBITED: {}",
                        p, err
                    );
                }
            }
        }
        if let Some(blocked) = super::helpers_path::check_cross_workspace_guard(proj_path, paths, state, allow_cross) {
            return format!("\n---\n{}", blocked);
        }
        let mut advisories = Vec::new();
        for p in &normalized_paths {
            if !proj_path.join(p).exists()
                && !super::gate_exemptions::is_exempt_from_loc_limit(p)
                && let Some(adv) = super::gate_edit_modularity::validate_new_file_modularity(p, justification)
            {
                advisories.push(adv);
            }
        }

        state.queue_deferred_authorization(&normalized_paths);
        let adv_section = if advisories.is_empty() {
            String::new()
        } else {
            format!("\n\n### Domain Cohesion Advisory (Soft Warning)\n{}", advisories.join("\n"))
        };
        format!(
            "\n---\n> [!NOTE] **Deferred Pre-Authorization Queued**: {} file(s) queued (Stage: '{}', Plan Approved: {}). Call `workflow_gate(action=\"approve_plan\", user_confirmed=true)` on plan approval.{}",
            normalized_paths.len(), state.workflow_stage, state.plan_approved, adv_section
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_deferred_read_auth_blocks_path_traversal() {
        let dir = std::env::temp_dir().join(format!("ag_auth_trav_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut state = ServerState::new();
        state.plan_approved = false;

        let res = evaluate_read_authorization(
            &json!({"operation": "read", "authorize": true}),
            &dir,
            &["../../etc/passwd".to_string()],
            &mut state,
        );
        assert!(res.contains("PATH_TRAVERSAL_PROHIBITED"));
        assert!(state.deferred_authorized_paths.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_auth_tip_emitted_once_per_session_and_skipped_when_authorized() {
        let dir = std::env::temp_dir().join(format!("ag_auth_tip_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut state = ServerState::new();
        let paths = vec!["src/main.rs".to_string()];

        let first = evaluate_read_authorization(&json!({"operation": "read"}), &dir, &paths, &mut state);
        assert!(first.contains("One-Turn Edit Authorization"));
        assert!(state.auth_tip_shown);

        let second = evaluate_read_authorization(&json!({"operation": "read"}), &dir, &paths, &mut state);
        assert!(second.is_empty());

        let mut auth_state = ServerState::new();
        auth_state.edit_authorized = true;
        let third = evaluate_read_authorization(&json!({"operation": "read"}), &dir, &paths, &mut auth_state);
        assert!(third.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
