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

    let advisories = super::gate_stage::collect_post_edit_quality_advisories(&proj_path, state);
    let advisory_section = if advisories.is_empty() {
        String::new()
    } else {
        format!(
            "\n\n## Post-Edit Quality & Complexity Advisory (Soft Limit)\n{}\n- Static Analysis Tip: Combine test verification with static linters (cargo clippy / eslint / ruff / golangci-lint).",
            advisories.join("\n")
        )
    };

    let v_cmd_raw = arguments
        .get("verification_command")
        .and_then(|v| v.as_str());
    let v_kw_raw = arguments
        .get("expected_output_keyword")
        .and_then(|k| k.as_str());

    let (mut safety_blockers, mut safety_advisories) =
        super::gate_stage::collect_post_edit_safety_findings(&proj_path, state);
    if let Some(cmd) = v_cmd_raw {
        if cmd.contains('\0') {
            safety_blockers.push(
                "- [BLOCKER] **VERIFY_COMMAND_INVALID_INPUT** (`verification_command`): Verification command contains null bytes.".to_string(),
            );
        }
        let cmd_report = crate::context::ast::analyze_verification_command(cmd);
        if let Some(blk) = cmd_report.format_blockers("verification_command") {
            safety_blockers.push(blk);
        }
        if let Some(adv) = cmd_report.format_advisories("verification_command") {
            safety_advisories.push(adv);
        }
    }

    if !safety_blockers.is_empty() {
        state.verification_passed = false;
        return Ok(format!(
            "# Verification Contract BLOCKED (TEST_SAFETY_VIOLATION)\n\n- Status: BLOCKED\n\nResolve the following test/data-safety blockers before running verification:\n{}\n",
            safety_blockers.join("\n")
        ));
    }

    let v_cmd = v_cmd_raw.map(str::trim).filter(|s| !s.is_empty());
    let v_kw = v_kw_raw.map(str::trim).filter(|s| !s.is_empty());

    let touches_test_or_persistence = state.modified_files.iter().any(|f| {
        let lower = f.to_lowercase();
        crate::context::ast::metrics::is_test_file_path(f)
            || ["storage", "store", "config", "state", "backup", "db"]
                .iter()
                .any(|kw| lower.contains(kw))
    });
    if touches_test_or_persistence {
        safety_advisories.push(
            "- Live-Data Immutability Check: Confirm tests run inside a temporary/in-memory sandbox and do not mutate real user/production files (e.g. APPDATA, ~/.config, live DB).".to_string(),
        );
    }

    let safety_section = if safety_advisories.is_empty() {
        String::new()
    } else {
        format!(
            "\n\n## Test-Safety & Data-Boundary Advisories\n{}",
            safety_advisories.join("\n")
        )
    };

    if let (Some(cmd), Some(kw)) = (v_cmd, v_kw) {
        state.verification_command = Some(cmd.to_string());
        state.expected_output_keyword = Some(kw.to_string());
        state.verification_passed = false;
        Ok(format!(
            "# Empirical Verification Contract Registered\n\n- Verification Command: `{}`\n- Expected Output Keyword: `{}`\n- Verification Status: REGISTERED (Awaiting test execution output)\n\nRun verification command to satisfy anti-hallucination requirement.{}{}",
            cmd, kw, advisory_section, safety_section
        ))
    } else {
        Ok(format!(
            "# Anti-Hallucination Post-Code Verification Checklist\n\n1. **Empirical Verification Required**: Trigger IDE/CLI `ask_question` tool to let user select verification test command (or confirm manual testing), then pass `verification_command` (e.g. 'cargo test') and `expected_output_keyword` (e.g. 'PASSED').\n2. **User Requirement Alignment**: Re-read the original user prompt and verify all explicitly requested features exist.\n3. **Zero Unverified Assumptions**: Base success strictly on empirical evidence, not speculative assumptions.{}{}",
            advisory_section, safety_section
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_verify_rejects_null_byte_and_ignores_blank_keyword_registration() {
        let dir = std::env::temp_dir().join(format!("ag_verify_unit_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);

        let mut state = ServerState::new();
        let blocked = handle_verify(
            &json!({
                "project_path": dir.to_string_lossy(),
                "verification_command": "cargo test\0evil",
                "expected_output_keyword": "ok"
            }),
            &mut state,
        )
        .unwrap();
        assert!(blocked.contains("VERIFY_COMMAND_INVALID_INPUT"));
        assert!(state.verification_command.is_none());

        let blank_kw = handle_verify(
            &json!({
                "project_path": dir.to_string_lossy(),
                "verification_command": "cargo test",
                "expected_output_keyword": "   "
            }),
            &mut state,
        )
        .unwrap();
        assert!(blank_kw.contains("Post-Code Verification Checklist"));
        assert!(state.verification_command.is_none());
        assert!(state.expected_output_keyword.is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
