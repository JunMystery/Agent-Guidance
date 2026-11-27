use serde_json::Value;
use std::path::Path;

use crate::mcp::state::ServerState;
use super::helpers::{is_valid_architecture_pattern, resolve_architecture_pattern};

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

    if user_msg.is_some_and(ServerState::is_negated_message) {
        if user_confirmed || state.plan_approved {
            state.plan_approved = false;
            return "# Workflow Gate: [approve_plan]\n\nStatus: BLOCKED | Error: USER_APPROVAL_NEGATED: Explicit negation detected in `user_message`. `user_confirmed=true` cannot override user rejection or pause.".to_string();
        }
        state.plan_approved = false;
    }

    let msg_approves = user_msg.map(ServerState::is_approval_message).unwrap_or(false);
    if !user_confirmed && !msg_approves && !state.plan_approved {
        return "# Workflow Gate: [approve_plan]\n\nStatus: BLOCKED | Error: USER_APPROVAL_REQUIRED: Plan approval cannot be self-granted by AI agents. You MUST present `implementation_plan.md` (`RequestFeedback: true`) in a standalone turn and wait for the user to explicitly click 'Proceed' or send an approval message. Never use `ask_question` answers as a substitute for plan approval.".to_string();
    }

    let has_explicit_arch = arguments.get("architecture_pattern").is_some();
    let raw_arch = arguments
        .get("architecture_pattern")
        .and_then(|a| a.as_str())
        .unwrap_or("Auto");
    let arch_pattern = resolve_architecture_pattern(raw_arch, proj_path, state);
    if (has_explicit_arch || !state.deferred_authorized_paths.is_empty())
        && !is_valid_architecture_pattern(&arch_pattern)
    {
        return format!(
            "# Workflow Gate: [approve_plan]\n\nStatus: BLOCKED (ARCHITECTURE_GATE_BLOCKED)\n- Declared Architecture: '{}'\n\nError: ARCHITECTURE_GATE_BLOCKED: Invalid `architecture_pattern`. Allowed: 'Clean_Architecture', 'Layered_Architecture', 'Package_By_Feature', 'Orchestrator', 'CLI_Pipeline', 'Flat_Library', or 'Auto'.",
            if arch_pattern.is_empty() { "NONE" } else { &arch_pattern }
        );
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
        if !is_valid_architecture_pattern(&arch_pattern) {
            return format!(
                "# Workflow Gate: [approve_plan]\n\nStatus: BLOCKED (ARCHITECTURE_GATE_BLOCKED)\n- Declared Architecture: '{}'\n\nError: ARCHITECTURE_GATE_BLOCKED: Invalid `architecture_pattern`.",
                if arch_pattern.is_empty() { "NONE" } else { &arch_pattern }
            );
        }

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
    state.clear_deferred_authorized_paths();
    let _ = state.set_stage("Build");
    if has_explicit_arch && is_valid_architecture_pattern(&arch_pattern) {
        state.active_architecture_pattern = Some(arch_pattern);
    }
    let _ = state.auto_checkpoint(proj_path);
    format!(
        "# Workflow Gate: [approve_plan]\n\nStatus: PASSED | Plan Approved: true | Stage: {}",
        state.workflow_stage
    )
}

fn has_ascii_word_boundary(haystack: &str, start: usize, end: usize) -> bool {
    let before_ok = haystack[..start]
        .chars()
        .next_back()
        .map(|c| !c.is_ascii_alphanumeric() && c != '_')
        .unwrap_or(true);
    let after_ok = haystack[end..]
        .chars()
        .next()
        .map(|c| !c.is_ascii_alphanumeric() && c != '_')
        .unwrap_or(true);
    before_ok && after_ok
}

fn is_zero_count_negated(lower: &str, kw_start: usize, kw_end: usize) -> bool {
    let prefix = lower[..kw_start].trim_end();
    if prefix.ends_with("0")
        && prefix
            .strip_suffix('0')
            .and_then(|s| s.chars().next_back())
            .is_none_or(|c| !c.is_ascii_digit())
    {
        return true;
    }
    if let Some(before_tests) = prefix.strip_suffix("tests") {
        let bt = before_tests.trim_end();
        if bt.ends_with('0')
            && bt
                .strip_suffix('0')
                .and_then(|s| s.chars().next_back())
                .is_none_or(|c| !c.is_ascii_digit())
        {
            return true;
        }
    }
    for neg in ["no", "not", "without", "never"] {
        if let Some(before) = prefix.strip_suffix(neg)
            && before
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_')
        {
            return true;
        }
    }
    let suffix = lower[kw_end..].trim_start_matches(|c: char| c.is_whitespace() || c == ':' || c == '=');
    if suffix.starts_with('0')
        && suffix
            .as_bytes()
            .get(1)
            .is_none_or(|b| !b.is_ascii_digit())
    {
        return true;
    }
    false
}

pub(crate) fn output_indicates_failure(output: &str) -> bool {
    if output.contains("error[E") {
        return true;
    }
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Passing cargo test line (e.g. `test my_failed_case ... ok`) is not a failure.
        if trimmed.starts_with("test ") && trimmed.ends_with("... ok") {
            continue;
        }

        // Case-sensitive `FAIL ` / `FAIL:` / `FAIL\t`
        let mut f_pos = 0;
        while let Some(idx) = trimmed[f_pos..].find("FAIL") {
            let abs = f_pos + idx;
            let after = abs + 4;
            let before_ok = trimmed[..abs]
                .chars()
                .next_back()
                .is_none_or(|c| c.is_whitespace() || c == '[' || c == '(');
            let next_ch = trimmed[after..].chars().next();
            if before_ok && matches!(next_ch, Some(' ' | '\t' | ':')) {
                return true;
            }
            f_pos = after;
        }

        let lower = trimmed.to_lowercase();
        // `0 passed` / `0 tests passed` (not `10 passed`)
        for needle in ["0 passed", "0 tests passed"] {
            let mut pos = 0;
            while let Some(idx) = lower[pos..].find(needle) {
                let abs = pos + idx;
                let end = abs + needle.len();
                if has_ascii_word_boundary(&lower, abs, end) {
                    let prefix = lower[..abs].trim_end();
                    if !prefix.ends_with("not") {
                        return true;
                    }
                }
                pos = end;
            }
        }

        // `failures:` unless explicitly zero (`failures: 0`)
        let mut fl_pos = 0;
        while let Some(idx) = lower[fl_pos..].find("failures:") {
            let abs = fl_pos + idx;
            let end = abs + "failures:".len();
            let before_ok = lower[..abs]
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_');
            if before_ok {
                let rest = lower[end..].trim_start();
                let is_zero = rest.starts_with('0')
                    && rest.as_bytes().get(1).is_none_or(|b| !b.is_ascii_digit());
                if !is_zero {
                    return true;
                }
            }
            fl_pos = end;
        }

        // `failed` unless negated / zero count
        let mut fd_pos = 0;
        while let Some(idx) = lower[fd_pos..].find("failed") {
            let abs = fd_pos + idx;
            let end = abs + "failed".len();
            if has_ascii_word_boundary(&lower, abs, end) && !is_zero_count_negated(&lower, abs, end)
            {
                return true;
            }
            fd_pos = end;
        }
    }
    false
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

    if let Some(actual) = output
        && output_indicates_failure(actual)
    {
        state.verification_passed = false;
        let _ = state.auto_checkpoint(proj_path);
        return "# Workflow Gate: [pass_verification]\n\nStatus: BLOCKED (VERIFICATION_OUTPUT_FAILED)\n\nVerification output contains failure indicators (`0 passed`, `failed`, `failures:`, `error[E`, or `FAIL`). Fix the failing checks and re-run verification before calling `pass_verification`.".to_string();
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
    let arch_pattern = resolve_architecture_pattern(raw_arch, proj_path, state);
    if !is_valid_architecture_pattern(&arch_pattern) {
        return format!(
            "# Architecture Pattern Locked\n\n- Status: BLOCKED (ARCHITECTURE_GATE_BLOCKED)\n- Declared Architecture: '{}'\n\nError: ARCHITECTURE_GATE_BLOCKED: Invalid `architecture_pattern`. Allowed: 'Clean_Architecture', 'Layered_Architecture', 'Package_By_Feature', 'Orchestrator', 'CLI_Pipeline', 'Flat_Library', or 'Auto'.",
            if arch_pattern.is_empty() { "NONE" } else { &arch_pattern }
        );
    }
    state.update_project_path(proj_path);
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
                for f in &restored {
                    state.ast_cache.invalidate_file(f);
                }
                let _ = state.drain_dirty_files();
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
