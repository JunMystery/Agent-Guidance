use serde_json::Value;
use crate::mcp::state::ServerState;

pub(crate) fn handle_verify(
    arguments: &Value,
    state: &mut ServerState,
) -> Result<String, (i32, String)> {
    let proj_path_arg = arguments
        .get("project_path")
        .and_then(|p| p.as_str())
        .unwrap_or(".");
    let proj_path = super::helpers::detect_project_path(proj_path_arg, state);

    if let Err((file_rel, loc)) = super::gate_stage::check_post_edit_300_loc_trap(&proj_path, state) {
        return Ok(format!(
            "# Anti-Hallucination Verification: BLOCKED (300_LOC_CAP_EXCEEDED)\n\n\
            - Target File: `{}` (Current Length: **{} lines** / hard limit: 300 lines)\n\n\
            **Error: 300_LOC_LIMIT_EXCEEDED**: Newly created or modified code file '{}' has {} lines, which breaches the 300 LOC hard ceiling.\n\
            You cannot verify or complete this task until this file is decomposed into modular sub-modules (< 150 LOC each).\n\n\
            **Action Required**: Decompose `{}` into focused sub-modules via BATCH `workflow_gate(action=\"authorize_edit\", relative_paths=[\"{}\", \"<path_to_new_submodule>\"], justification=\"Refactor/Decompose: ...\")`.",
            file_rel, loc, file_rel, loc, file_rel, file_rel
        ));
    }

    let v_cmd = arguments
        .get("verification_command")
        .and_then(|v| v.as_str());
    let v_kw = arguments
        .get("expected_output_keyword")
        .and_then(|k| k.as_str());

    if let (Some(cmd), Some(kw)) = (v_cmd, v_kw) {
        state.verification_command = Some(cmd.to_string());
        state.expected_output_keyword = Some(kw.to_string());
        state.verification_passed = false;
        Ok(format!(
            "# Empirical Verification Contract Registered\n\n- Verification Command: `{}`\n- Expected Output Keyword: `{}`\n- Verification Status: REGISTERED (Awaiting test execution output)\n\nRun verification command to satisfy anti-hallucination requirement.",
            cmd, kw
        ))
    } else {
        Ok(format!(
            "# Anti-Hallucination Post-Code Verification Checklist\n\n1. **Empirical Verification Required**: Trigger IDE/CLI `ask_question` tool to let user select verification test command (or confirm manual testing), then pass `verification_command` (e.g. 'cargo test') and `expected_output_keyword` (e.g. 'PASSED').\n2. **User Requirement Alignment**: Re-read the original user prompt and verify all explicitly requested features exist.\n3. **Zero Unverified Assumptions**: Base success strictly on empirical evidence, not speculative assumptions."
        ))
    }
}
