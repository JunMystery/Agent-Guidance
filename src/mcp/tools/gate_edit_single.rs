use std::path::Path;

use crate::mcp::impact::{assess_file_risk, create_file_snapshot, RiskLevel};
use crate::mcp::state::ServerState;
use super::gate_edit::is_exempt_from_loc_limit;
use super::gate_edit_modularity::validate_new_file_modularity;
use super::gate_stage::count_file_lines;
use super::helpers::validate_path_scoped;

#[derive(Debug, Clone)]
pub(crate) struct SingleFileAuthOutcome {
    pub rel_path: String,
    pub is_passed: bool,
    pub is_new: bool,
    pub target_loc: usize,
    pub risk_level: RiskLevel,
    pub dependent_count: usize,
    pub error_msg: Option<String>,
    pub warning: Option<String>,
    pub co_change_tip: Option<String>,
    pub quality_advisory: Option<String>,
}

pub(crate) fn evaluate_single_file_auth(
    proj_path: &Path,
    rel_path: &str,
    justification: &str,
    _arch_pattern: &str,
    allow_cross: bool,
    state: &mut ServerState,
) -> SingleFileAuthOutcome {
    let mut outcome = SingleFileAuthOutcome {
        rel_path: rel_path.to_string(),
        is_passed: false,
        is_new: false,
        target_loc: 0,
        risk_level: RiskLevel::Low,
        dependent_count: 0,
        error_msg: None,
        warning: None,
        co_change_tip: None,
        quality_advisory: None,
    };

    let full_target_path = match validate_path_scoped(proj_path, rel_path, allow_cross) {
        Ok(p) => p,
        Err(err_msg) => {
            outcome.error_msg = Some(format!("PATH_TRAVERSAL_PROHIBITED: {}", err_msg));
            return outcome;
        }
    };

    let impact = assess_file_risk(proj_path, rel_path);
    outcome.risk_level = impact.risk_level.clone();
    outcome.dependent_count = impact.dependent_count;
    outcome.warning = impact.warning;

    outcome.is_new = !full_target_path.exists();
    outcome.target_loc = if full_target_path.exists() && full_target_path.is_file() {
        count_file_lines(&full_target_path)
    } else {
        0
    };

    let is_exempt = is_exempt_from_loc_limit(rel_path);
    if outcome.is_new && !is_exempt {
        outcome.quality_advisory = validate_new_file_modularity(rel_path, justification);
    } else if !outcome.is_new && !is_exempt {
        let ast_adv = crate::context::ast::analyze_file_path(&full_target_path, rel_path)
            .and_then(|report| report.format_advisory(rel_path));
        let safety_adv = crate::context::ast::analyze_safety_path(&full_target_path, rel_path)
            .and_then(|report| report.format_all(rel_path));
        outcome.quality_advisory = match (ast_adv, safety_adv) {
            (Some(a), Some(b)) => Some(format!("{}\n{}", a, b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
    }

    if impact.risk_level == RiskLevel::High
        && (justification.trim().len() < 10 || justification == "No justification provided")
    {
        outcome.error_msg = Some(format!(
            "HIGH_RISK_JUSTIFICATION_REQUIRED: Critical Hub (>8 callers). Justification explaining test mitigation required for `{}`.",
            rel_path
        ));
        return outcome;
    }

    // Success path: Record modified file, invalidate prior verification, take snapshot, and check coupling
    state.record_modified_file(rel_path);
    state.verification_passed = false;
    let _ = create_file_snapshot(proj_path, rel_path, &state.session_id);
    outcome.is_passed = true;

    if let Ok(db) = crate::context::db::CodeGraphDb::open_for_project(proj_path) {
        if let Ok(coupled) = crate::context::co_change::predict_coupled_files(
            &db.conn,
            &[rel_path.to_string()],
            1,
            0.6,
        ) {
            if let Some(top) = coupled.first() {
                let cross_dir = Path::new(&top.coupled_with).parent() != Path::new(&top.file).parent();
                let domain_hint = if cross_dir {
                    " Cross-directory co-change detected: consider colocating files serving the same domain/use case to narrow change blast radius."
                } else {
                    ""
                };
                outcome.co_change_tip = Some(format!(
                    "Historically `{}` is modified together with `{}` ({} co-changes, {:.0}% confidence).{}",
                    top.coupled_with, top.file, top.co_change_count, top.confidence * 100.0, domain_hint
                ));
            }
        }
    }

    outcome
}
