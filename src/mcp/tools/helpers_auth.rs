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
    let allow_cross = arguments
        .get("allow_cross_workspace")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if state.plan_approved {
        if state.workflow_stage == "Plan" {
            let _ = state.set_stage("Build");
        }
        if state.workflow_stage != "Build" {
            return format!(
                "\n---\n# Edit Approval Gate Authorization\n\n- Status: BLOCKED\n- Active Stage: {}\n\nError: WORKFLOW_STAGE_BLOCKED: Edits require `Build` stage (current stage: `{}`). Advance to `Build` before authorizing edits.",
                state.workflow_stage, state.workflow_stage
            );
        }
        let raw_arch = arguments
            .get("architecture_pattern")
            .and_then(|a| a.as_str())
            .unwrap_or("Auto");
        let arch_pattern = super::helpers::resolve_architecture_pattern(raw_arch, proj_path, state);
        if !matches!(
            arch_pattern.as_str(),
            "Clean_Architecture"
                | "Layered_Architecture"
                | "Package_By_Feature"
                | "Orchestrator"
                | "CLI_Pipeline"
                | "Flat_Library"
        ) {
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
        if let Some(blocked) = super::helpers_path::check_cross_workspace_guard(proj_path, paths, state, allow_cross) {
            return format!("\n---\n{}", blocked);
        }
        let mut advisories = Vec::new();
        for p in paths {
            if !proj_path.join(p).exists()
                && !super::gate_exemptions::is_exempt_from_loc_limit(p)
                && let Some(adv) = super::gate_edit_modularity::validate_new_file_modularity(p, justification)
            {
                advisories.push(adv);
            }
        }

        state.queue_deferred_authorization(paths);
        let adv_section = if advisories.is_empty() {
            String::new()
        } else {
            format!("\n\n### Domain Cohesion Advisory (Soft Warning)\n{}", advisories.join("\n"))
        };
        format!(
            "\n---\n> [!NOTE] **Deferred Pre-Authorization Queued**:\n> {} file(s) queued for automatic authorization upon plan approval.\n> Active Stage: '{}' | Plan Approved: {}.\n> Call `workflow_gate(action=\"approve_plan\", user_confirmed=true)` once plan is confirmed to activate authorization in 1 turn.{}",
            paths.len(), state.workflow_stage, state.plan_approved, adv_section
        )
    }
}
