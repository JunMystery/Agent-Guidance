use std::path::Path;

use crate::context::db::CodeGraphDb;
use crate::mcp::impact::{assess_file_risk, create_file_snapshot, RiskLevel};
use crate::mcp::state::ServerState;
use super::gate_edit::is_exempt_from_loc_limit;
use super::gate_edit_modularity::validate_new_file_modularity;
use super::gate_stage::count_file_lines;
use super::helpers::validate_path_scoped_normalized;

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
    pub snapshot_note: String,
    pub co_change_tip: Option<String>,
    pub quality_advisory: Option<String>,
}

pub(crate) fn evaluate_single_file_auth(
    proj_path: &Path,
    rel_path: &str,
    justification: &str,
    arch_pattern: &str,
    allow_cross: bool,
    state: &mut ServerState,
) -> SingleFileAuthOutcome {
    let db = CodeGraphDb::open_for_project(proj_path).ok();
    evaluate_single_file_auth_with_db(
        proj_path,
        rel_path,
        justification,
        arch_pattern,
        allow_cross,
        state,
        db.as_ref(),
    )
}

pub(crate) fn evaluate_single_file_auth_with_db(
    proj_path: &Path,
    rel_path: &str,
    justification: &str,
    _arch_pattern: &str,
    allow_cross: bool,
    state: &mut ServerState,
    db: Option<&CodeGraphDb>,
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
        snapshot_note: "No snapshot".to_string(),
        co_change_tip: None,
        quality_advisory: None,
    };

    let (full_target_path, norm_rel_path) =
        match validate_path_scoped_normalized(proj_path, rel_path, allow_cross) {
            Ok(pair) => pair,
            Err(err_msg) => {
                outcome.error_msg = Some(format!("PATH_TRAVERSAL_PROHIBITED: {}", err_msg));
                return outcome;
            }
        };

    if norm_rel_path.is_empty() {
        outcome.error_msg = Some("RELATIVE_PATH_REQUIRED: Target file path cannot resolve to workspace root.".to_string());
        return outcome;
    }

    outcome.rel_path = norm_rel_path.clone();

    let impact = assess_file_risk(proj_path, &norm_rel_path);
    outcome.risk_level = impact.risk_level.clone();
    outcome.dependent_count = impact.dependent_count;
    outcome.warning = impact.warning;

    outcome.is_new = !full_target_path.exists();
    outcome.target_loc = if full_target_path.exists() && full_target_path.is_file() {
        count_file_lines(&full_target_path)
    } else {
        0
    };

    let is_exempt = is_exempt_from_loc_limit(&norm_rel_path);
    if outcome.is_new && !is_exempt {
        outcome.quality_advisory = validate_new_file_modularity(&norm_rel_path, justification);
    } else if !outcome.is_new && !is_exempt {
        let ast_adv = crate::context::ast::analyze_file_path(&full_target_path, &norm_rel_path)
            .and_then(|report| report.format_advisory(&norm_rel_path));
        let safety_adv = crate::context::ast::analyze_safety_path(&full_target_path, &norm_rel_path)
            .and_then(|report| report.format_all(&norm_rel_path));
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
            norm_rel_path
        ));
        return outcome;
    }

    // Success path: Record modified file, invalidate prior verification, take snapshot, and check coupling
    state.record_modified_file(&norm_rel_path);
    if state.is_fast_path {
        state.record_fast_path_file(&norm_rel_path);
    }
    state.verification_passed = false;
    outcome.snapshot_note = match create_file_snapshot(proj_path, &norm_rel_path, &state.session_id)
    {
        Ok(true) => "Snapshot created".to_string(),
        Ok(false) => {
            if outcome.is_new {
                "New file (no snapshot needed)".to_string()
            } else {
                "Snapshot skipped or already exists".to_string()
            }
        }
        Err(e) => format!("Snapshot error ({})", e),
    };
    outcome.is_passed = true;

    if let Some(db_ref) = db {
        if let Ok(coupled) = crate::context::co_change::predict_coupled_files(
            &db_ref.conn,
            std::slice::from_ref(&norm_rel_path),
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
