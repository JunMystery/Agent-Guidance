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
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let op = arguments.get("operation").and_then(|v| v.as_str()).unwrap_or("");
    let is_auth_op = op == "read_and_authorize" || op == "read_auth";

    if !authorize_requested && !is_auth_op {
        if !paths.is_empty() {
            let gate_paths_json = serde_json::to_string(paths).unwrap_or_else(|_| "[]".to_string());
            return format!(
                "\n---\n> [!TIP] **One-Turn Edit Authorization**:\n> Authorize edits in 1 turn before editing:\n> `workflow_gate(action=\"authorize_edit\", project_path=\"{}\", relative_paths={}, risk_level=\"LOW\", justification=\"Implement planned modifications\", architecture_pattern=\"Auto\")`",
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

    if state.plan_approved {
        if state.workflow_stage == "Plan" {
            let _ = state.set_stage("Build");
        }
        let raw_arch = arguments
            .get("architecture_pattern")
            .and_then(|a| a.as_str())
            .unwrap_or("Auto");
        let arch_pattern = super::helpers::resolve_architecture_pattern(raw_arch, proj_path, state);
        let auth_report = super::gate_edit_batch::handle_batch_authorize(
            proj_path, paths, justification, &arch_pattern, state,
        );
        format!("\n---\n## Direct Edit Authorization Report\n\n{}", auth_report)
    } else {
        let mut validation_err = None;
        for p in paths {
            if !proj_path.join(p).exists()
                && let Err(e) = super::gate_edit_modularity::validate_new_file_modularity(p, justification)
            {
                validation_err = Some(e);
                break;
            }
        }

        if let Some(err_msg) = validation_err {
            format!("\n---\n## Pre-Authorization Validation Blocked\n\n{}", err_msg)
        } else {
            state.queue_deferred_authorization(paths);
            format!(
                "\n---\n> [!NOTE] **Deferred Pre-Authorization Queued**:\n> {} file(s) queued for automatic authorization upon plan approval.\n> Active Stage: '{}' | Plan Approved: {}.\n> Call `workflow_gate(action=\"approve_plan\", user_confirmed=true)` once plan is confirmed to activate authorization in 1 turn.",
                paths.len(), state.workflow_stage, state.plan_approved
            )
        }
    }
}
