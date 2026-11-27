use serde_json::Value;
use std::path::Path;

use crate::mcp::state::ServerState;
use super::helpers::resolve_architecture_pattern;

pub(crate) fn validate_approval_not_question_response(
    user_msg: Option<&str>,
    justification: &str,
) -> Option<String> {
    if let Some(msg) = user_msg {
        let msg_trimmed = msg.trim();
        if msg_trimmed.starts_with("A1:")
            || msg_trimmed.starts_with("A:")
            || msg_trimmed.contains("\nA1:")
            || msg_trimmed.contains("\nA2:")
        {
            return Some("# Workflow Gate: [approve_plan]\n\nStatus: BLOCKED | Error: QUESTION_RESPONSE_NOT_PLAN_APPROVAL: Answering an `ask_question` prompt (/grill-me) is design feedback, NOT plan approval. Synthesize decisions into `implementation_plan.md` (`RequestFeedback: true`) in a standalone turn and wait for explicit user approval (Proceed button).".to_string());
        }
    }

    let just_lower = justification.to_lowercase();
    if just_lower.contains("chọn phương án")
        || just_lower.contains("chọn đáp án")
        || just_lower.contains("ask_question")
        || just_lower.contains("trả lời câu hỏi")
        || just_lower.contains("answered question")
        || just_lower.contains("question choice")
        || just_lower.contains("selected option")
    {
        let has_explicit_approval = user_msg
            .map(|m| {
                let m_lower = m.to_lowercase();
                m_lower.contains("proceed")
                    || m_lower.contains("approved")
                    || m_lower.contains("duyệt")
                    || m_lower.contains("đồng ý")
                    || m_lower.contains("chấp nhận")
            })
            .unwrap_or(false);

        if !has_explicit_approval {
            return Some("# Workflow Gate: [approve_plan]\n\nStatus: BLOCKED | Error: QUESTION_CHOICE_NOT_PLAN_APPROVAL: Answering an `ask_question` prompt (/grill-me) is design clarification, NOT plan approval. Present `implementation_plan.md` (`RequestFeedback: true`) in a standalone turn (without calling ask_question) and wait for the user to explicitly click 'Proceed' or send plan approval.".to_string());
        }
    }

    None
}

pub(crate) fn handle_approve_plan(
    arguments: &Value,
    state: &mut ServerState,
    user_confirmed: bool,
    user_msg: Option<&str>,
    proj_path: &Path,
) -> String {
    let justification = arguments
        .get("justification")
        .and_then(|j| j.as_str())
        .unwrap_or("");

    if let Some(blocked) = validate_approval_not_question_response(user_msg, justification) {
        state.plan_approved = false;
        return blocked;
    }

    let msg_approves = user_msg.map(ServerState::is_approval_message).unwrap_or(false);
    if !user_confirmed && !msg_approves && !state.plan_approved {
        return "# Workflow Gate: [approve_plan]\n\nStatus: BLOCKED | Error: USER_APPROVAL_REQUIRED: Plan approval cannot be self-granted by AI agents. You MUST present `implementation_plan.md` (`RequestFeedback: true`) in a standalone turn and wait for the user to explicitly click 'Proceed' or send an approval message. Never use `ask_question` answers as a substitute for plan approval.".to_string();
    }

    let mut paths = super::gate_edit::extract_target_paths(arguments);
    for dp in &state.deferred_authorized_paths {
        if !paths.contains(dp) {
            paths.push(dp.clone());
        }
    }

    let (auto_paths, plan_source) =
        super::gate_plan_parser::auto_discover_plan_paths(proj_path, arguments, state);
    let mut auto_count = 0;
    for ap in auto_paths {
        if !paths.contains(&ap) {
            paths.push(ap);
            auto_count += 1;
        }
    }

    if !paths.is_empty() {
        let allow_cross = arguments
            .get("allow_cross_workspace")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if let Some(blocked_msg) = super::helpers_path::check_cross_workspace_guard(
            proj_path,
            &paths,
            state,
            allow_cross,
        ) {
            return blocked_msg;
        }

        state.approve_plan();
        state.clear_deferred_authorized_paths();
        let _ = state.set_stage("Build");
        let raw_arch = arguments
            .get("architecture_pattern")
            .and_then(|a| a.as_str())
            .unwrap_or("Auto");
        let arch_pattern = resolve_architecture_pattern(raw_arch, proj_path, state);
        let justification = arguments
            .get("justification")
            .and_then(|j| j.as_str())
            .unwrap_or("Pre-authorized during implementation plan approval");

        let batch_report = super::gate_edit_batch::handle_batch_authorize(
            proj_path,
            &paths,
            justification,
            &arch_pattern,
            allow_cross,
            state,
        );

        let auto_note = if auto_count > 0 {
            format!(
                "\nPlan Artifact Auto-Auth: {} file(s) automatically discovered from `{}`.",
                auto_count,
                plan_source
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "implementation_plan.md".to_string())
            )
        } else {
            String::new()
        };

        return format!(
            "# Workflow Gate: [approve_plan & pre-authorized]\n\nStatus: PASSED | Plan Approved: true | Stage: {}\nBatch Pre-Authorization: {} file(s) processed.{}\n\n{}",
            state.workflow_stage,
            paths.len(),
            auto_note,
            batch_report
        );
    }

    state.approve_plan();
    let _ = state.auto_checkpoint(proj_path);
    format!(
        "# Workflow Gate: [approve_plan]\n\nStatus: PASSED | Plan Approved: true | Stage: {}",
        state.workflow_stage
    )
}

pub(crate) fn handle_pass_verification(
    arguments: &Value,
    state: &mut ServerState,
    proj_path: &Path,
) -> String {
    let output = arguments
        .get("verification_output")
        .or_else(|| arguments.get("command_output"))
        .and_then(|v| v.as_str());

    if let Some(expected_kw) = state
        .expected_output_keyword
        .as_deref()
        .map(str::trim)
        .filter(|k| !k.is_empty())
    {
        let matched = output.map(|actual| actual.contains(expected_kw)).unwrap_or(false);
        if !matched {
            state.verification_passed = false;
            let _ = state.auto_checkpoint(proj_path);
            return format!(
                "# Workflow Gate: [pass_verification]\n\nStatus: BLOCKED (VERIFICATION_KEYWORD_MISSING) | Expected Keyword: `{}`\n\nVerification output was missing or did not match the registered contract keyword.",
                expected_kw
            );
        }
    } else if state.verification_command.is_some()
        && output.map(|actual| actual.trim().is_empty()).unwrap_or(true)
    {
        state.verification_passed = false;
        let _ = state.auto_checkpoint(proj_path);
        return "# Workflow Gate: [pass_verification]\n\nStatus: BLOCKED (VERIFICATION_OUTPUT_MISSING)\n\nVerification command is registered, so non-empty `verification_output` is required to confirm passing verification.".to_string();
    }

    state.verification_passed = true;
    state.fix_attempts = 0;
    let _ = state.auto_checkpoint(proj_path);
    format!(
        "# Workflow Gate: [pass_verification]\n\nStatus: PASSED | Plan Approved: {} | Stage: {} | Verification Passed: true | Fix Attempts: 0\n\nVerification passed and recorded in session checkpoint.",
        state.plan_approved, state.workflow_stage
    )
}

pub(crate) fn handle_set_architecture(
    arguments: &Value,
    state: &mut ServerState,
    proj_path: &Path,
) -> String {
    let raw_arch = arguments
        .get("architecture_pattern")
        .or_else(|| arguments.get("pattern"))
        .and_then(|a| a.as_str())
        .unwrap_or("Auto");
    state.update_project_path(proj_path);
    let arch_pattern = resolve_architecture_pattern(raw_arch, proj_path, state);
    state.active_architecture_pattern = Some(arch_pattern.clone());
    let _ = ServerState::save_persisted_architecture(proj_path, &arch_pattern);
    let _ = state.auto_checkpoint(proj_path);
    format!(
        "# Architecture Pattern Locked\n\n- Project: {}\n- Confirmed Architecture: {}\n- Persistence: Saved to `.agent-context/architecture.json`\n\nPattern memorized for all workflow stages and future sessions.",
        proj_path.display(),
        arch_pattern
    )
}

pub(crate) fn handle_rollback(
    state: &mut ServerState,
    proj_path: &Path,
) -> String {
    match crate::mcp::impact::restore_session_snapshots(proj_path, &state.session_id) {
        Ok(restored) => {
            if restored.is_empty() {
                format!(
                    "# Rollback Guard: [rollback]\n\nNo snapshot files found for session '{}'. No changes were reverted.",
                    state.session_id
                )
            } else {
                state.verification_passed = false;
                state.modified_files.clear();
                let _ = state.auto_checkpoint(proj_path);
                format!(
                    "# Rollback Guard: [rollback] SUCCESS\n\nSuccessfully restored {} file(s) to their pre-edit state for session '{}':\n\n{}",
                    restored.len(),
                    state.session_id,
                    restored.iter().map(|f| format!("- `{}`", f)).collect::<Vec<_>>().join("\n")
                )
            }
        }
        Err(e) => format!("# Rollback Guard: [rollback] ERROR\n\nFailed to restore session snapshots: {}", e),
    }
}
