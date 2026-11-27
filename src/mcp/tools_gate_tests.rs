use super::*;

#[test]
fn test_auto_architecture_gate_authorization() {
    let mut state = ServerState::new();
    state.plan_approved = true;
    state.set_stage("Build").unwrap();

    // 1. Authorize edit with 'Auto' pattern should succeed and resolve pattern
    let res = handle_tool_call(
        "workflow_gate",
        json!({
            "action": "authorize_edit",
            "relative_path": "src/main.rs",
            "architecture_pattern": "Auto",
            "risk_level": "LOW",
            "justification": "Refactoring test"
        }),
        &mut state,
    );
    assert!(
        res.is_ok(),
        "workflow_gate authorize_edit with 'Auto' must succeed: {:?}",
        res
    );
    let text = res.unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(text.contains("Status: PASSED"));
    assert!(state.edit_authorized);
    assert!(state.active_architecture_pattern.is_some());

    // 2. Query precode guidance should contain active architecture
    let precode_res =
        handle_tool_call("guidance", json!({ "operation": "precode" }), &mut state);
    assert!(precode_res.is_ok());
    let precode_text = precode_res.unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(precode_text.contains("Architecture Pattern:"));
}

#[test]
fn test_workflow_gate_plan_approval_via_user_message() {
    let mut state = ServerState::new();
    assert!(!state.plan_approved);

    // 1. Unconfirmed approve_plan without user_message is BLOCKED
    let blocked_res = handle_tool_call(
        "workflow_gate",
        json!({ "action": "approve_plan" }),
        &mut state,
    );
    assert!(blocked_res.is_ok());
    assert!(!state.plan_approved);

    // 2. Calling workflow_gate approve_plan with user_confirmed=true sets plan_approved=true
    let res = handle_tool_call(
        "workflow_gate",
        json!({ "action": "approve_plan", "user_confirmed": true }),
        &mut state,
    );
    assert!(res.is_ok());
    assert!(state.plan_approved);

    // 3. set_stage to Build now succeeds
    let stage_res = handle_tool_call(
        "workflow_gate",
        json!({ "action": "set_stage", "target_stage": "Build" }),
        &mut state,
    );
    assert!(stage_res.is_ok());
    let text = stage_res.unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(text.contains("Status: PASSED"));
    assert_eq!(state.workflow_stage, "Build");
}

#[test]
fn test_workflow_gate_zero_turn_advance_and_impact_guard() {
    let temp_dir = std::env::temp_dir().join(format!("ag_impact_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(temp_dir.join("src"));
    let core_file = temp_dir.join("src/core.rs");
    std::fs::write(&core_file, "pub struct CoreConfig;\n").unwrap();

    let mut state = ServerState::new();
    state.workflow_stage = "Plan".to_string();
    state.plan_approved = true;

    // 1. Zero-Turn Predictive Transition (Plan -> Build on authorize_edit)
    let auth_res = handle_tool_call(
        "workflow_gate",
        json!({
            "action": "authorize_edit",
            "project_path": temp_dir.to_str().unwrap(),
            "relative_path": "src/core.rs",
            "architecture_pattern": "CLI_Pipeline",
            "justification": "Modifying core config with unit tests"
        }),
        &mut state,
    );
    assert!(auth_res.is_ok());
    let text = auth_res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(text.contains("Status: PASSED"));
    assert_eq!(state.workflow_stage, "Build");
    assert!(state.edit_authorized);

    // 2. Modify file content
    std::fs::write(&core_file, "pub struct CoreConfig;\npub fn new_modified_code() {}\n").unwrap();

    // 3. Rollback Guard restores file
    let rollback_res = handle_tool_call(
        "workflow_gate",
        json!({
            "action": "rollback",
            "project_path": temp_dir.to_str().unwrap()
        }),
        &mut state,
    );
    assert!(rollback_res.is_ok());
    let rollback_text = rollback_res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(rollback_text.contains("Successfully restored 1 file(s)"));

    // Verify original content was restored
    let restored_content = std::fs::read_to_string(&core_file).unwrap();
    assert_eq!(restored_content, "pub struct CoreConfig;\n");

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_workflow_gate_soft_warns_300_loc_and_complex_functions() {
    let temp_dir = std::env::temp_dir().join(format!("loc300_soft_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_dir);

    let target_file = temp_dir.join("LargeService.kt");
    let lines: Vec<String> = (1..=350).map(|i| format!("// Line {}", i)).collect();
    std::fs::write(&target_file, lines.join("\n")).unwrap();

    let mut state = ServerState::new();
    state.plan_approved = true;
    state.set_stage("Build").unwrap();

    let res = handle_tool_call(
        "workflow_gate",
        json!({
            "action": "authorize_edit",
            "relative_path": "LargeService.kt",
            "justification": "Add new user authentication logic",
            "architecture_pattern": "Clean_Architecture",
            "project_path": temp_dir.to_str().unwrap()
        }),
        &mut state,
    );
    assert!(res.is_ok());
    let text = res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(text.contains("Status: PASSED"));
    assert!(!text.contains("BLOCKED"));
    assert!(text.contains("Soft File LOC Notice"));
    assert!(text.contains("unrelated responsibilities"));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_workflow_gate_and_verify_surface_ast_function_complexity() {
    let temp_dir = std::env::temp_dir().join(format!("ast_complex_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_dir);

    let target_file = temp_dir.join("ComplexHandler.rs");
    let mut lines = vec![
        "pub fn deeply_nested(a: i32, b: i32, c: i32, d: i32, e: i32) -> i32 {".to_string(),
        "    if a > 0 { for i in 0..b { if i > c { while d > 0 { if e > 1 { return 1; } } } } }".to_string(),
    ];
    for i in 1..=62 {
        lines.push(format!("    let step_{} = {};", i, i));
    }
    lines.push("    0\n}".to_string());
    std::fs::write(&target_file, lines.join("\n")).unwrap();

    let mut state = ServerState::new();
    state.plan_approved = true;
    state.set_stage("Build").unwrap();

    let auth_res = handle_tool_call(
        "workflow_gate",
        json!({
            "action": "authorize_edit",
            "relative_path": "ComplexHandler.rs",
            "justification": "Inspect complexity advisory on existing handler",
            "architecture_pattern": "CLI_Pipeline",
            "project_path": temp_dir.to_str().unwrap()
        }),
        &mut state,
    );
    assert!(auth_res.is_ok());
    let auth_text = auth_res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(auth_text.contains("Status: PASSED"));
    assert!(auth_text.contains("Function `deeply_nested`"));
    assert!(auth_text.contains("cyclomatic="));
    assert!(auth_text.contains("nesting="));

    let verify_res = handle_tool_call(
        "guidance",
        json!({
            "operation": "verify",
            "verification_command": "cargo test",
            "expected_output_keyword": "ok",
            "project_path": temp_dir.to_str().unwrap()
        }),
        &mut state,
    );
    assert!(verify_res.is_ok());
    let verify_text = verify_res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(verify_text.contains("Empirical Verification Contract Registered"));
    assert!(verify_text.contains("Post-Edit Quality & Complexity Advisory"));
    assert!(verify_text.contains("deeply_nested"));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_workflow_gate_blocks_missing_relative_path() {
    let mut state = ServerState::new();
    state.plan_approved = true;
    state.set_stage("Build").unwrap();

    // Calling authorize_edit without relative_path must be strictly BLOCKED
    let res = handle_tool_call(
        "workflow_gate",
        json!({
            "action": "authorize_edit",
            "architecture_pattern": "Clean_Architecture",
            "justification": "General feature build"
        }),
        &mut state,
    );
    assert!(res.is_ok());
    let text = res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(text.contains("BLOCKED (RELATIVE_PATH_REQUIRED)"));
    assert!(text.contains("RELATIVE_PATH_REQUIRED"));
    assert!(text.contains("300 LOC"));
}

#[test]
fn test_workflow_gate_new_file_modular_architecture_mandate() {
    let temp_dir = std::env::temp_dir().join(format!("ag_newfile_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_dir);

    let mut state = ServerState::new();
    state.plan_approved = true;
    state.set_stage("Build").unwrap();

    // Authorize a non-existent (new) file
    let res = handle_tool_call(
        "workflow_gate",
        json!({
            "action": "authorize_edit",
            "project_path": temp_dir.to_str().unwrap(),
            "relative_path": "frontend/src/views/ProcurementView.tsx",
            "architecture_pattern": "Layered_Architecture",
            "justification": "Build new procurement view and modals"
        }),
        &mut state,
    );
    assert!(res.is_ok());
    let text = res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(text.contains("Status: PASSED"));
    assert!(text.contains("[NEW FILE]"));
    assert!(text.contains("300 LOC soft limit"));
    assert!(text.contains("Layered_Architecture Architecture"));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_pass_verification_sets_flag_and_enforces_exit_gate() {
    let temp_dir = std::env::temp_dir().join(format!("ag_verify_gate_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_dir);

    let mut state = ServerState::new();
    state.plan_approved = true;
    state.set_stage("Build").unwrap();

    handle_tool_call(
        "guidance",
        json!({
            "operation": "verify",
            "project_path": temp_dir.to_str().unwrap(),
            "verification_command": "npm test",
            "expected_output_keyword": "VERIFY_OK"
        }),
        &mut state,
    )
    .unwrap();
    assert!(!state.verification_passed);

    // Lowercase "proposal" via set_stage must be blocked before pass_verification
    let blocked = handle_tool_call(
        "workflow_gate",
        json!({
            "action": "set_stage",
            "target_stage": "proposal",
            "project_path": temp_dir.to_str().unwrap()
        }),
        &mut state,
    )
    .unwrap();
    assert!(blocked["content"][0]["text"].as_str().unwrap().contains("VERIFICATION_NOT_PASSED"));

    // Mismatched output stays blocked
    let bad_pass = handle_tool_call(
        "workflow_gate",
        json!({
            "action": "pass_verification",
            "verification_output": "FAIL: 1 failed",
            "project_path": temp_dir.to_str().unwrap()
        }),
        &mut state,
    )
    .unwrap();
    assert!(bad_pass["content"][0]["text"].as_str().unwrap().contains("VERIFICATION_KEYWORD_MISSING"));
    assert!(!state.verification_passed);

    // Matching output marks verification_passed=true and unlocks Proposal
    let good_pass = handle_tool_call(
        "workflow_gate",
        json!({
            "action": "pass_verification",
            "verification_output": "All ok\nVERIFY_OK",
            "project_path": temp_dir.to_str().unwrap()
        }),
        &mut state,
    )
    .unwrap();
    assert!(good_pass["content"][0]["text"].as_str().unwrap().contains("Status: PASSED"));
    assert!(state.verification_passed);

    let adv = handle_tool_call(
        "workflow_gate",
        json!({
            "action": "advance",
            "target_stage": "proposal",
            "project_path": temp_dir.to_str().unwrap()
        }),
        &mut state,
    )
    .unwrap();
    assert_eq!(state.workflow_stage, "Proposal");
    assert!(!adv["content"][0]["text"].as_str().unwrap().contains("BLOCKED"));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_fix_stage_edit_auth_unknown_action_arch_validation_and_cache_invalidation() {
    let temp_dir = std::env::temp_dir().join(format!("ag_gate_hardening_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(temp_dir.join("src")).unwrap();
    let lib_file = temp_dir.join("src/lib.rs");
    std::fs::write(&lib_file, "pub fn answer() -> i32 { 42 }\n").unwrap();

    let mut state = ServerState::new();
    state.plan_approved = true;
    state.set_stage("Fix").unwrap();

    // 1. Unknown action returns -32602
    let unknown_err = handle_tool_call(
        "workflow_gate",
        json!({ "action": "nonexistent_action" }),
        &mut state,
    )
    .unwrap_err();
    assert_eq!(unknown_err.0, -32602);

    // 2. Invalid architecture_pattern blocked across set_architecture, approve_plan, advance, authorize_edit
    for args in [
        json!({"action": "set_architecture", "architecture_pattern": "Spaghetti", "project_path": temp_dir.to_str().unwrap()}),
        json!({"action": "approve_plan", "user_confirmed": true, "architecture_pattern": "Spaghetti", "project_path": temp_dir.to_str().unwrap()}),
        json!({"action": "advance", "target_stage": "Build", "architecture_pattern": "Spaghetti", "project_path": temp_dir.to_str().unwrap()}),
        json!({"action": "authorize_edit", "relative_path": "src/lib.rs", "architecture_pattern": "Spaghetti", "project_path": temp_dir.to_str().unwrap()}),
    ] {
        let res = handle_tool_call("workflow_gate", args.clone(), &mut state).unwrap();
        let text = res["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("ARCHITECTURE_GATE_BLOCKED"), "Expected arch block for {:?}, got: {}", args, text);
    }

    // 3. Edit authorization succeeds in Fix stage
    let fix_auth = handle_tool_call(
        "workflow_gate",
        json!({
            "action": "authorize_edit",
            "relative_path": "src/lib.rs",
            "architecture_pattern": "Flat_Library",
            "justification": "Fix bug in lib",
            "project_path": temp_dir.to_str().unwrap()
        }),
        &mut state,
    )
    .unwrap();
    assert!(fix_auth["content"][0]["text"].as_str().unwrap().contains("Status: PASSED"));

    // 4. Proposal exit gate blocks modified code file even without verification_command
    let prop_blocked = handle_tool_call(
        "workflow_gate",
        json!({
            "action": "advance",
            "target_stage": "Proposal",
            "project_path": temp_dir.to_str().unwrap()
        }),
        &mut state,
    )
    .unwrap();
    assert!(prop_blocked["content"][0]["text"].as_str().unwrap().contains("VERIFICATION_NOT_PASSED"));

    // 5. pass_verification rejects failure outputs and allows zero-failure outputs
    for bad_out in [
        "test result: ok. 0 passed; 0 failed",
        "0 tests passed",
        "test result: FAILED. 1 passed; 1 failed",
        "failures:\n    tests::broken",
        "error[E0425]: cannot find value",
        "FAIL src/lib.test.ts",
    ] {
        let res = handle_tool_call(
            "workflow_gate",
            json!({
                "action": "pass_verification",
                "verification_output": bad_out,
                "project_path": temp_dir.to_str().unwrap()
            }),
            &mut state,
        )
        .unwrap();
        assert!(
            res["content"][0]["text"].as_str().unwrap().contains("VERIFICATION_OUTPUT_FAILED"),
            "Expected failure rejection for: {}",
            bad_out
        );
        assert!(!state.verification_passed);
    }

    for ok_out in [
        "test result: ok. 12 passed; 0 failed; 0 ignored",
        "Tests: 0 failed, 10 passed",
        "Failures: 0",
    ] {
        let res = handle_tool_call(
            "workflow_gate",
            json!({
                "action": "pass_verification",
                "verification_output": ok_out,
                "project_path": temp_dir.to_str().unwrap()
            }),
            &mut state,
        )
        .unwrap();
        assert!(
            res["content"][0]["text"].as_str().unwrap().contains("Status: PASSED"),
            "Expected pass for: {}",
            ok_out
        );
        assert!(state.verification_passed);
    }

    // 6. invalidate and rollback invalidate ast_cache
    let cache_key = crate::mcp::tools::context_cache::AstCacheKey {
        path: lib_file.canonicalize().unwrap().to_string_lossy().replace('\\', "/"),
        target_symbol: None,
        view_mode: "auto".to_string(),
        start_line: None,
        end_line: None,
        show_line_numbers: false,
    };
    state.ast_cache.insert(cache_key.clone(), "cached".to_string(), &lib_file, 10);
    assert_eq!(state.ast_cache.len(), 1);
    handle_tool_call(
        "workflow_gate",
        json!({"action": "invalidate", "relative_path": "src/lib.rs", "project_path": temp_dir.to_str().unwrap()}),
        &mut state,
    )
    .unwrap();
    assert_eq!(state.ast_cache.len(), 0);

    state.ast_cache.insert(cache_key, "cached2".to_string(), &lib_file, 10);
    std::fs::write(&lib_file, "pub fn answer() -> i32 { 99 }\n").unwrap();
    handle_tool_call(
        "workflow_gate",
        json!({"action": "rollback", "project_path": temp_dir.to_str().unwrap()}),
        &mut state,
    )
    .unwrap();
    assert_eq!(state.ast_cache.len(), 0);

    let _ = std::fs::remove_dir_all(&temp_dir);
}
