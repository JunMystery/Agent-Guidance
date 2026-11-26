use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::mcp::state::ServerState;
use super::gate_edit::is_exempt_from_loc_limit;
use super::helpers::resolve_architecture_pattern;

pub(crate) fn count_file_lines(path: &Path) -> usize {
    if let Ok(file) = std::fs::File::open(path) {
        BufReader::new(file).lines().count()
    } else {
        0
    }
}

pub(crate) fn collect_post_edit_quality_advisories(
    proj_path: &Path,
    state: &ServerState,
) -> Vec<String> {
    let mut advisories = Vec::new();
    for file_rel in &state.modified_files {
        if is_exempt_from_loc_limit(file_rel) {
            continue;
        }
        let full = proj_path.join(file_rel);
        if let Some(report) = crate::context::ast::analyze_file_path(&full, file_rel) {
            if let Some(adv) = report.format_advisory(file_rel) {
                advisories.push(adv);
            }
        }
    }
    advisories
}

pub(crate) fn collect_post_edit_safety_findings(
    proj_path: &Path,
    state: &ServerState,
) -> (Vec<String>, Vec<String>) {
    let mut blockers = Vec::new();
    let mut advisories = Vec::new();
    for file_rel in &state.modified_files {
        if is_exempt_from_loc_limit(file_rel) {
            continue;
        }
        let full = proj_path.join(file_rel);
        if let Some(report) = crate::context::ast::analyze_safety_path(&full, file_rel) {
            if let Some(blk) = report.format_blockers(file_rel) {
                blockers.push(blk);
            }
            if let Some(adv) = report.format_advisories(file_rel) {
                advisories.push(adv);
            }
        }
    }
    (blockers, advisories)
}

pub(crate) fn is_proposal_or_review_stage(stage: &str) -> bool {
    matches!(
        stage.trim().to_lowercase().as_str(),
        "proposal" | "review" | "document" | "proposal/review"
    )
}

pub(crate) fn check_proposal_exit_gates(
    action: &str,
    target_stage: &str,
    proj_path: &Path,
    state: &ServerState,
) -> Result<String, String> {
    if !is_proposal_or_review_stage(target_stage) {
        return Ok(String::new());
    }

    if state.verification_command.is_some() && !state.verification_passed {
        return Err(format!(
            "# Workflow Gate: [{}]\n\nStatus: BLOCKED (VERIFICATION_NOT_PASSED)\n\nVerification contract (`{}`) has not been confirmed as passing. Run verification and call `workflow_gate(action=\"pass_verification\")` before advancing to `{}`.",
            action,
            state.verification_command.as_deref().unwrap_or(""),
            target_stage
        ));
    }

    let (safety_blockers, safety_advisories) = collect_post_edit_safety_findings(proj_path, state);
    if !safety_blockers.is_empty() {
        return Err(format!(
            "# Workflow Gate: [{}]\n\nStatus: BLOCKED (TEST_SAFETY_VIOLATION)\n\nResolve blocking test-safety violations before advancing to `{}`:\n{}\n\nUse `patch.object`/exact imported module path, or add `# test-safety-allow: mock-namespace` only if explicitly verified.",
            action,
            target_stage,
            safety_blockers.join("\n")
        ));
    }

    let advisories = collect_post_edit_quality_advisories(proj_path, state);
    let mut out = String::new();
    if !advisories.is_empty() {
        out.push_str(&format!(
            "\n\n## Post-Edit Quality & Complexity Advisory (Soft Limit)\n{}\n- Tip: Run your language static linter (e.g. cargo clippy, eslint, ruff, golangci-lint) alongside unit tests.",
            advisories.join("\n")
        ));
    }
    if !safety_advisories.is_empty() {
        out.push_str(&format!(
            "\n\n## Test-Safety & Data-Boundary Advisories\n{}",
            safety_advisories.join("\n")
        ));
    }
    Ok(out)
}

pub(crate) fn handle_set_stage(
    state: &mut ServerState,
    stage_target: Option<&str>,
    inline_approved: bool,
    proj_path: &Path,
) -> String {
    if let Some(target) = stage_target {
        let post_edit_advisory = match check_proposal_exit_gates("set_stage", target, proj_path, state) {
            Ok(adv) => adv,
            Err(blocked_msg) => return blocked_msg,
        };
        if target.eq_ignore_ascii_case("build") && inline_approved {
            state.approve_plan();
        }
        match state.set_stage(target) {
            Ok(new_stage) => {
                let _ = state.auto_checkpoint(proj_path);
                format!(
                    "# Workflow Gate: [set_stage]\n\nStatus: PASSED | Stage Changed To: {} | Plan Approved: {} | Fix Attempts: {}{}",
                    new_stage, state.plan_approved, state.fix_attempts, post_edit_advisory
                )
            }
            Err(err_msg) => {
                let _ = state.auto_checkpoint(proj_path);
                format!(
                    "# Workflow Gate: [set_stage]\n\nStatus: BLOCKED | Error: {}. If user approved via GUI or chat, pass user_confirmed=true (or trigger ask_question only if interactive clarification needed).",
                    err_msg
                )
            }
        }
    } else {
        "# Workflow Gate: [set_stage]\n\nStatus: BLOCKED | Error: target_stage argument is required for set_stage action.".to_string()
    }
}

pub(crate) fn handle_advance(
    arguments: &Value,
    state: &mut ServerState,
    inline_approved: bool,
    proj_path: &Path,
) -> String {
    let target_stage = arguments
        .get("target_stage")
        .and_then(|t| t.as_str())
        .unwrap_or("Build");

    let post_edit_advisory = match check_proposal_exit_gates("advance", target_stage, proj_path, state) {
        Ok(adv) => adv,
        Err(blocked_msg) => return blocked_msg,
    };

    let raw_arch = arguments
        .get("architecture_pattern")
        .and_then(|a| a.as_str())
        .unwrap_or("Auto");
    let risk_level = arguments
        .get("risk_level")
        .and_then(|r| r.as_str())
        .unwrap_or("LOW");
    let arch_pattern = resolve_architecture_pattern(raw_arch, proj_path, state);
    let paths = super::gate_edit::extract_target_paths(arguments);
    let allow_cross = arguments
        .get("allow_cross_workspace")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let will_be_build = target_stage.eq_ignore_ascii_case("build");
    let will_be_approved = state.plan_approved || (will_be_build && inline_approved);
    if !paths.is_empty() && will_be_build && will_be_approved {
        if let Some(blocked_msg) =
            super::gate_edit::enforce_edit_boundaries(proj_path, &paths, allow_cross, state)
        {
            if !state.plan_approved {
                let _ = state.auto_checkpoint(proj_path);
            }
            return blocked_msg;
        }
    }

    if will_be_build && inline_approved {
        state.approve_plan();
    }
    state.last_risk_level = Some(risk_level.to_string());
    let stage_res = state.set_stage(target_stage);

    if matches!(
        arch_pattern.as_str(),
        "Clean_Architecture"
            | "Layered_Architecture"
            | "Package_By_Feature"
            | "Orchestrator"
            | "CLI_Pipeline"
            | "Flat_Library"
    ) && state.workflow_stage == "Build"
        && state.plan_approved
    {
        state.active_architecture_pattern = Some(arch_pattern.clone());
    }

    match stage_res {
        Ok(msg) => {
            if !paths.is_empty() && state.workflow_stage == "Build" && state.plan_approved {
                let justification = arguments
                    .get("justification")
                    .and_then(|j| j.as_str())
                    .unwrap_or("Pre-authorized during stage advance to Build");
                let batch_report = super::gate_edit_batch::handle_batch_authorize(
                    proj_path,
                    &paths,
                    justification,
                    &arch_pattern,
                    allow_cross,
                    state,
                );
                format!(
                    "# Workflow Gate: [advance & pre-authorized]\n\n{}\n- Edit Authorized: {}\n- Architecture Pattern: {}\nBatch Pre-Authorization: {} file(s) processed.\n\n{}{}",
                    msg,
                    state.edit_authorized,
                    arch_pattern,
                    paths.len(),
                    batch_report,
                    post_edit_advisory
                )
            } else {
                let _ = state.auto_checkpoint(proj_path);
                format!(
                    "# Workflow Gate: [advance]\n\n{}\n- Edit Authorized: {}\n- Architecture Pattern: {}{}",
                    msg,
                    state.edit_authorized,
                    state
                        .active_architecture_pattern
                        .as_deref()
                        .unwrap_or("NONE"),
                    post_edit_advisory
                )
            }
        }
        Err(err) => format!("# Workflow Gate: [advance]\n\nError: {}", err),
    }
}

pub(crate) fn handle_check(state: &ServerState) -> String {
    let status_str = if state.workflow_stage == "Build" && !state.plan_approved {
        "BLOCKED"
    } else {
        "PASSED"
    };
    let mut resp = format!(
        "# Workflow Gate: [check]\n\nStatus: {} | Plan Approved: {} | Stage: {} | Fix Attempts: {}",
        status_str, state.plan_approved, state.workflow_stage, state.fix_attempts
    );
    if state.workflow_stage == "Build" && !state.plan_approved {
        resp.push_str("\n\nPlan approval required before editing code. If user approved via GUI/chat, invoke workflow_gate with user_confirmed=true (or ask_question if interactive approval needed).");
    }
    if state.workflow_stage == "Test_Recheck" {
        resp.push_str("\n\n**ANTI-HALLUCINATION ENFORCER ACTIVE**: Re-read the original user prompt & verify all requested features against real build/test outputs before declaring task complete.");
    }
    resp
}

pub(crate) fn handle_status(state: &ServerState) -> String {
    let edit_allowed = state.workflow_stage == "Build" && state.plan_approved && state.edit_authorized;
    format!(
        "# Workflow Stage Status\n\n- Active Stage: {}\n- Plan Approved: {}\n- Fix Attempts: {}/3\n- Edit Authorized: {}",
        state.workflow_stage, state.plan_approved, state.fix_attempts, edit_allowed
    )
}
