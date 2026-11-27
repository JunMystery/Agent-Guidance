use serde_json::Value;

use crate::mcp::state::ServerState;
use super::gate_approval::{
    handle_approve_plan, handle_pass_verification, handle_rollback, handle_set_architecture,
    validate_approval_not_question_response,
};
use super::gate_edit::handle_authorize_edit;
use super::gate_stage::{handle_advance, handle_check, handle_set_stage, handle_status};
use super::helpers::{detect_project_path_with_targets, ensure_not_cancelled};

pub(crate) fn handle(
    arguments: Value,
    state: &mut ServerState,
) -> Result<String, (i32, String)> {
    ensure_not_cancelled(state)?;
    let action = arguments
        .get("action")
        .and_then(|a| a.as_str())
        .unwrap_or("check");
    let stage_target = arguments
        .get("target_stage")
        .or_else(|| arguments.get("stage"))
        .and_then(|s| s.as_str());
    let user_confirmed = arguments
        .get("user_confirmed")
        .or_else(|| arguments.get("confirmed"))
        .or_else(|| arguments.get("plan_approved"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let user_msg = arguments.get("user_message").and_then(|u| u.as_str());

    let can_accept_inline_approval = matches!(action, "set_stage" | "advance" | "advance_stage");
    let mut inline_approved = false;
    if can_accept_inline_approval && (user_confirmed || user_msg.is_some()) {
        let justification = arguments
            .get("justification")
            .and_then(|j| j.as_str())
            .unwrap_or("");
        if let Some(blocked) = validate_approval_not_question_response(user_msg, justification) {
            state.plan_approved = false;
            return Ok(blocked);
        }
        if user_msg.is_some_and(ServerState::is_negated_message) {
            state.plan_approved = false;
            if user_confirmed {
                return Ok("# Workflow Gate: [approval]\n\nStatus: BLOCKED | Error: USER_APPROVAL_NEGATED: Explicit negation detected in `user_message`. `user_confirmed=true` cannot override user rejection or pause.".to_string());
            }
            inline_approved = false;
        } else {
            let msg_approves = user_msg.map(ServerState::is_approval_message).unwrap_or(false);
            inline_approved = user_confirmed || msg_approves;
        }
    }

    let proj_path_arg = arguments
        .get("project_path")
        .and_then(|p| p.as_str())
        .unwrap_or(".");
    let target_paths = super::gate_edit::extract_target_paths(&arguments);
    let proj_path = detect_project_path_with_targets(proj_path_arg, &target_paths, state);

    let resp = match action {
        "check" => handle_check(state),
        "approve" | "approve_plan" => {
            handle_approve_plan(&arguments, state, user_confirmed, user_msg, &proj_path)
        }
        "pass_verification" => handle_pass_verification(&arguments, state, &proj_path),
        "set_stage" => handle_set_stage(state, stage_target, inline_approved, &proj_path),
        "status" => handle_status(state),
        "set_architecture" => handle_set_architecture(&arguments, state, &proj_path),
        "advance" | "advance_stage" => handle_advance(&arguments, state, inline_approved, &proj_path),
        "authorize_edit" => handle_authorize_edit(&arguments, state),
        "invalidate" | "sync_file" => {
            let rel_path = arguments
                .get("relative_path")
                .and_then(|r| r.as_str())
                .unwrap_or("");
            if rel_path.is_empty() {
                "# Workflow Gate: [invalidate]\n\nError: 'relative_path' is required for invalidate/sync_file action.".to_string()
            } else {
                state.ast_cache.invalidate_file(rel_path);
                match crate::context::indexer::invalidator::invalidate_and_sync_file(&proj_path, rel_path) {
                    Ok(rep) => {
                        state.ast_cache.invalidate_file(&rep.rel_path);
                        format!(
                            "# Workflow Gate: [invalidate]\n\n- File: `{}`\n- Reindexed: {}\n- Symbols: {}\n- Edges: {}\n- Duration: {}ms\n\nWrite-through AST sync complete.",
                            rep.rel_path, rep.reindexed, rep.symbols_extracted, rep.edges_created, rep.duration_ms
                        )
                    }
                    Err(e) => format!("# Workflow Gate: [invalidate]\n\nError syncing file `{}`: {}", rel_path, e),
                }
            }
        }
        "rollback" => handle_rollback(state, &proj_path),
        _ => {
            return Err((
                -32602,
                format!(
                    "Unknown workflow_gate action '{}'. Valid actions: check, status, set_stage, set_architecture, advance, advance_stage, approve_plan, approve, pass_verification, authorize_edit, invalidate, sync_file, rollback.",
                    action
                ),
            ));
        }
    };
    Ok(resp)
}