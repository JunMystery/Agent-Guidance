use super::*;

#[test]
fn test_workflow_gate_batch_authorize_edit() {
    let mut state = ServerState::new();
    state.plan_approved = true;
    state.set_stage("Build").unwrap();

    let res = handle_tool_call(
        "workflow_gate",
        json!({
            "action": "authorize_edit",
            "project_path": ".",
            "relative_paths": ["src/main.rs", "Cargo.toml"],
            "architecture_pattern": "Auto",
            "risk_level": "LOW",
            "justification": "Refactoring test batch"
        }),
        &mut state,
    );

    assert!(res.is_ok(), "Batch authorize_edit must succeed: {:?}", res);
    let text = res.unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(text.contains("Batch Edit Authorization Gate"));
    assert!(text.contains("PASSED (2/2 authorized)"));
    assert!(text.contains("`src/main.rs`"));
    assert!(text.contains("`Cargo.toml`"));
    assert!(state.edit_authorized);
    assert!(state.modified_files.contains(&"src/main.rs".to_string()));
    assert!(state.modified_files.contains(&"Cargo.toml".to_string()));
}

#[test]
fn test_workflow_gate_batch_authorize_with_violations() {
    let mut state = ServerState::new();
    state.plan_approved = true;
    state.set_stage("Build").unwrap();

    let res = handle_tool_call(
        "workflow_gate",
        json!({
            "action": "authorize_edit",
            "project_path": ".",
            "relative_paths": ["src/main.rs", "../../../evil.rs"],
            "architecture_pattern": "Auto",
            "risk_level": "LOW",
            "justification": "Testing partial batch"
        }),
        &mut state,
    );

    assert!(res.is_ok());
    let text = res.unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(text.contains("PARTIAL_PASSED (1/2 authorized, 1 blocked)"));
    assert!(text.contains("PATH_TRAVERSAL_PROHIBITED"));
    assert!(state.modified_files.contains(&"src/main.rs".to_string()));
    assert!(!state.modified_files.contains(&"../../../evil.rs".to_string()));
}

#[test]
fn test_workflow_gate_approve_plan_with_batch_preauth() {
    let mut state = ServerState::new();
    assert!(!state.plan_approved);
    assert_eq!(state.workflow_stage, "Context");

    let res = handle_tool_call(
        "workflow_gate",
        json!({
            "action": "approve_plan",
            "project_path": ".",
            "user_confirmed": true,
            "relative_paths": ["src/main.rs", "Cargo.toml"],
            "architecture_pattern": "Auto"
        }),
        &mut state,
    );

    assert!(res.is_ok(), "approve_plan with relative_paths must succeed: {:?}", res);
    let text = res.unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(text.contains("approve_plan & pre-authorized"));
    assert!(text.contains("Plan Approved: true"));
    assert_eq!(state.workflow_stage, "Build");
    assert!(state.plan_approved);
    assert!(state.edit_authorized);
    assert!(state.modified_files.contains(&"src/main.rs".to_string()));
}

#[test]
fn test_workflow_gate_batch_empty_array() {
    let mut state = ServerState::new();
    state.plan_approved = true;
    state.set_stage("Build").unwrap();

    let res = handle_tool_call(
        "workflow_gate",
        json!({
            "action": "authorize_edit",
            "project_path": ".",
            "relative_paths": [],
            "justification": "Empty array test"
        }),
        &mut state,
    );

    assert!(res.is_ok());
    let text = res.unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(text.contains("NO_FILES_SPECIFIED") || text.contains("RELATIVE_PATH_REQUIRED"));
}

#[test]
fn test_workflow_gate_advance_with_batch_preauth() {
    let mut state = ServerState::new();
    state.set_stage("Plan").unwrap();

    let res = handle_tool_call(
        "workflow_gate",
        json!({
            "action": "advance",
            "target_stage": "Build",
            "user_confirmed": true,
            "relative_paths": ["src/main.rs", "Cargo.toml"],
            "architecture_pattern": "Auto",
            "project_path": "."
        }),
        &mut state,
    );

    assert!(res.is_ok());
    let text = res.unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(text.contains("advance & pre-authorized"));
    assert_eq!(state.workflow_stage, "Build");
    assert!(state.plan_approved);
    assert!(state.edit_authorized);
    assert!(state.modified_files.contains(&"src/main.rs".to_string()));
}

#[test]
fn test_workflow_gate_batch_zero_authorized_keeps_edit_authorized_false() {
    let mut state = ServerState::new();
    state.plan_approved = true;
    state.set_stage("Build").unwrap();
    state.edit_authorized = false;

    let res = handle_tool_call(
        "workflow_gate",
        json!({
            "action": "authorize_edit",
            "project_path": ".",
            "relative_paths": ["../../../evil1.rs", "../../../evil2.rs"],
            "architecture_pattern": "Auto",
            "justification": "Testing all blocked batch"
        }),
        &mut state,
    )
    .unwrap();
    let text = res["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("BLOCKED (0/2 authorized)"));
    assert!(!state.edit_authorized);
}

#[test]
fn test_pass_verification_requires_output_and_authorize_edit_resets_flag() {
    let temp = std::env::temp_dir().join(format!("ag_pv_req_{}", std::process::id()));
    let _ = std::fs::create_dir_all(temp.join("src"));
    std::fs::write(temp.join("src/a.rs"), "pub fn a() {}\n").unwrap();

    let mut state = ServerState::new();
    state.plan_approved = true;
    state.set_stage("Build").unwrap();

    handle_tool_call(
        "guidance",
        json!({
            "operation": "verify",
            "project_path": temp.to_str().unwrap(),
            "verification_command": "npm test",
            "expected_output_keyword": "VERIFY_OK"
        }),
        &mut state,
    )
    .unwrap();

    let missing_out = handle_tool_call(
        "workflow_gate",
        json!({
            "action": "pass_verification",
            "project_path": temp.to_str().unwrap()
        }),
        &mut state,
    )
    .unwrap();
    assert!(missing_out["content"][0]["text"].as_str().unwrap().contains("VERIFICATION_KEYWORD_MISSING"));
    assert!(!state.verification_passed);

    let pass_res = handle_tool_call(
        "workflow_gate",
        json!({
            "action": "pass_verification",
            "project_path": temp.to_str().unwrap(),
            "verification_output": "done\nVERIFY_OK"
        }),
        &mut state,
    )
    .unwrap();
    assert!(pass_res["content"][0]["text"].as_str().unwrap().contains("Status: PASSED"));
    assert!(state.verification_passed);

    handle_tool_call(
        "workflow_gate",
        json!({
            "action": "authorize_edit",
            "project_path": temp.to_str().unwrap(),
            "relative_path": "src/a.rs",
            "architecture_pattern": "Flat_Library",
            "justification": "Subsequent edit invalidates prior verification"
        }),
        &mut state,
    )
    .unwrap();
    assert!(!state.verification_passed);

    let _ = std::fs::remove_dir_all(&temp);
}

#[test]
fn test_pending_skill_proposals_and_advance_boundary_guards() {
    let temp = std::env::temp_dir().join(format!("ag_adv_guard_{}", std::process::id()));
    let _ = std::fs::create_dir_all(temp.join("src"));
    std::fs::write(temp.join("src/a.rs"), "pub fn a() {}\n").unwrap();
    std::fs::write(temp.join("src/b.rs"), "pub fn b() {}\n").unwrap();

    let mut state = ServerState::new();
    state.priority_gate_pass();
    state.workflow_stage = "Plan".to_string();
    state.pending_skill_proposals = vec![("agent-guidance".to_string(), "skills/agent-guidance/SKILL.md".to_string(), 1.0)];

    for (tool, args) in [
        ("workflow_gate", json!({"action": "advance", "target_stage": "Build", "user_confirmed": true})),
        ("workflow_gate", json!({"action": "approve_plan", "user_confirmed": true})),
        ("project_context", json!({"operation": "read", "relative_path": "src/a.rs", "authorize": true})),
    ] {
        let err = state.can_call_tool(tool, &args).unwrap_err();
        assert!(err.contains("SKILL_PROPOSAL_PENDING"), "Expected block for {}: {}", tool, err);
    }

    state.pending_skill_proposals.clear();
    state.plan_approved = true;
    state.edit_authorized = false;
    handle_tool_call(
        "workflow_gate",
        json!({"action": "advance", "target_stage": "Build", "architecture_pattern": "Flat_Library", "project_path": temp.to_str().unwrap()}),
        &mut state,
    )
    .unwrap();
    assert!(!state.edit_authorized, "advance without target paths must not grant blanket edit_authorized");

    state.is_fast_path = true;
    let blocked = handle_tool_call(
        "workflow_gate",
        json!({
            "action": "advance",
            "target_stage": "Build",
            "relative_paths": ["src/a.rs", "src/b.rs"],
            "project_path": temp.to_str().unwrap()
        }),
        &mut state,
    )
    .unwrap();
    assert!(blocked["content"][0]["text"].as_str().unwrap().contains("FAST_PATH_BOUNDARY_EXCEEDED"));
    assert!(!state.plan_approved);

    let _ = std::fs::remove_dir_all(&temp);
}

