use super::*;

#[test]
fn test_session_continuity_learn_and_handoff() {
    let temp_dir = std::env::temp_dir().join(format!("ag_learn_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_dir);
    let mut state = ServerState::new();

    // 1. Record Learning with Category
    let learn_res = handle_tool_call(
        "session_continuity",
        json!({
            "operation": "learn",
            "project_path": temp_dir.to_str().unwrap(),
            "category": "build_test",
            "learning": "Always run cargo test with mock database pool"
        }),
        &mut state,
    );
    assert!(learn_res.is_ok());
    let text = learn_res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(text.contains("Project Learning Saved"));

    // 2. Generate Handoff Protocol
    let handoff_res = handle_tool_call(
        "session_continuity",
        json!({
            "operation": "handoff",
            "project_path": temp_dir.to_str().unwrap(),
            "next_action": "Run cargo test and inspect failure in auth module"
        }),
        &mut state,
    );
    assert!(handoff_res.is_ok());
    let handoff_text = handoff_res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(handoff_text.contains("Cross-Agent Handoff Protocol"));

    // 3. Verify handoff file on disk
    let handoff_file = temp_dir.join(".agent-context/handoff.md");
    assert!(handoff_file.exists());

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_auto_checkpoint_on_stage_advance_and_edit() {
    let temp_dir = std::env::temp_dir().join(format!("tools_auto_cp_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let _ = std::fs::create_dir_all(&temp_dir);

    let mut state = ServerState::new();

    // 1. Advance to Plan
    let plan_res = handle_tool_call(
        "workflow_gate",
        json!({
            "action": "set_stage",
            "target_stage": "Plan",
            "project_path": temp_dir.to_str().unwrap()
        }),
        &mut state,
    );
    assert!(plan_res.is_ok());

    let cp_file = temp_dir
        .join(".agent-context")
        .join("sessions")
        .join(format!("{}.json", state.session_id));
    assert!(cp_file.exists(), "Checkpoint should exist after set_stage to Plan");

    // 2. Approve plan
    let app_res = handle_tool_call(
        "workflow_gate",
        json!({
            "action": "approve",
            "user_confirmed": true,
            "project_path": temp_dir.to_str().unwrap()
        }),
        &mut state,
    );
    assert!(app_res.is_ok());

    // 3. Advance to Build
    let adv_res = handle_tool_call(
        "workflow_gate",
        json!({
            "action": "advance",
            "target_stage": "Build",
            "architecture_pattern": "Layered_Architecture",
            "project_path": temp_dir.to_str().unwrap()
        }),
        &mut state,
    );
    assert!(adv_res.is_ok());

    let loaded = ServerState::load_from_dir(&temp_dir).unwrap();
    assert_eq!(loaded.workflow_stage, "Build");
    assert!(loaded.plan_approved);
    assert_eq!(
        loaded.active_architecture_pattern.as_deref(),
        Some("Layered_Architecture")
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_session_diff_and_handoff_integration() {
    let temp_dir = std::env::temp_dir().join(format!("tools_diff_handoff_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let _ = std::fs::create_dir_all(&temp_dir);

    let mut state = ServerState::new();
    state.plan_approved = true;
    state.set_stage("Build").unwrap();

    // 1. Authorize edit on src/lib.rs
    let edit_res = handle_tool_call(
        "workflow_gate",
        json!({
            "action": "authorize_edit",
            "relative_path": "src/lib.rs",
            "architecture_pattern": "Layered_Architecture",
            "project_path": temp_dir.to_str().unwrap()
        }),
        &mut state,
    );
    assert!(edit_res.is_ok());
    assert_eq!(state.modified_files, vec!["src/lib.rs"]);

    // 2. Call session_continuity(operation="diff")
    let diff_res = handle_tool_call(
        "session_continuity",
        json!({
            "operation": "diff",
            "project_path": temp_dir.to_str().unwrap()
        }),
        &mut state,
    );
    assert!(diff_res.is_ok());
    let diff_text = diff_res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(diff_text.contains("Session Modification Summary"));
    assert!(diff_text.contains("src/lib.rs"));

    // 3. Call session_continuity(operation="handoff")
    let handoff_res = handle_tool_call(
        "session_continuity",
        json!({
            "operation": "handoff",
            "next_action": "Run cargo test and benchmark",
            "project_path": temp_dir.to_str().unwrap()
        }),
        &mut state,
    );
    assert!(handoff_res.is_ok());
    let handoff_text = handoff_res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(handoff_text.contains("Cross-Agent Handoff Protocol"));
    assert!(handoff_text.contains("src/lib.rs"));
    assert!(handoff_text.contains("Run cargo test and benchmark"));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_session_continuity_list_and_switch() {
    let temp_dir = std::env::temp_dir().join(format!("tools_list_switch_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let _ = std::fs::create_dir_all(&temp_dir);

    let mut target_state = ServerState::with_client_name(Some("cursor".to_string()));
    target_state.workflow_stage = "Build".to_string();
    target_state.plan_approved = true;
    target_state.edit_authorized = true;
    target_state.record_modified_file("src/parser.rs");
    assert!(target_state.save_to_dir(&temp_dir).is_ok());

    let mut active_state = ServerState::with_client_name(Some("antigravity".to_string()));

    // 1. List sessions
    let list_res = handle_tool_call(
        "session_continuity",
        json!({
            "operation": "list",
            "project_path": temp_dir.to_str().unwrap()
        }),
        &mut active_state,
    );
    assert!(list_res.is_ok());
    let list_text = list_res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(list_text.contains("Active & Archived Sessions"));
    assert!(list_text.contains(&target_state.session_id));
    assert!(list_text.contains("cursor"));

    // 2. Switch to target_state.session_id
    let switch_res = handle_tool_call(
        "session_continuity",
        json!({
            "operation": "switch",
            "session_id": target_state.session_id,
            "project_path": temp_dir.to_str().unwrap()
        }),
        &mut active_state,
    );
    assert!(switch_res.is_ok());
    let switch_text = switch_res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(switch_text.contains("Successfully switched active session"));

    // 3. Verify active_state inherited target session context with Zero-Trust reset
    assert_eq!(active_state.session_id, target_state.session_id);
    assert_eq!(active_state.workflow_stage, "Build");
    assert_eq!(active_state.modified_files, vec!["src/parser.rs"]);
    assert!(!active_state.edit_authorized, "Zero-Trust policy should reset edit_authorized");
    assert!(!active_state.plan_approved, "Zero-Trust policy should reset plan_approved");

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_session_switch_failed_lookup_preserves_state_and_resets_flags() {
    let temp_dir = std::env::temp_dir().join(format!("tools_switch_safe_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_dir);

    let mut target = ServerState::new();
    target.verification_passed = true;
    target.is_fast_path = true;
    target.queue_deferred_authorization(&["src/a.rs".to_string()]);
    target.save_to_dir(&temp_dir).unwrap();

    let mut active = ServerState::new();
    active.priority_gate_pass();
    active.workflow_stage = "Build".to_string();
    active.plan_approved = true;
    active.record_modified_file("src/keep.rs");

    let miss = crate::mcp::router::handle_request(
        "tools/call",
        Some(json!({
            "name": "session_continuity",
            "arguments": {
                "operation": "switch",
                "session_id": "nonexistent_session_999",
                "project_path": temp_dir.to_str().unwrap()
            }
        })),
        &mut active,
    )
    .unwrap();
    assert!(miss["content"][0]["text"].as_str().unwrap().contains("Failed to switch"));
    assert_eq!(active.modified_files, vec!["src/keep.rs"]);
    assert!(active.plan_approved);

    crate::mcp::router::handle_request(
        "tools/call",
        Some(json!({
            "name": "session_continuity",
            "arguments": {
                "operation": "switch",
                "session_id": target.session_id,
                "project_path": temp_dir.to_str().unwrap()
            }
        })),
        &mut active,
    )
    .unwrap();
    assert!(!active.verification_passed);
    assert!(!active.is_fast_path);
    assert!(active.deferred_authorized_paths.is_empty());

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_session_clear_scoped_to_active_and_save_no_double_count() {
    let temp_dir = std::env::temp_dir().join(format!("tools_sess_m12_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    // 1. Unknown operation returns Err((-32602, ...))
    let mut state_a = ServerState::new();
    let bad = handle_tool_call(
        "session_continuity",
        json!({"operation": "unknown_op", "project_path": temp_dir.to_str().unwrap()}),
        &mut state_a,
    );
    assert!(matches!(bad, Err((-32602, _))));

    // 2. Repeated save does not double-count co-changes
    state_a.record_modified_file("src/a.rs");
    state_a.record_modified_file("src/b.rs");
    let _ = handle_tool_call(
        "session_continuity",
        json!({"operation": "save", "project_path": temp_dir.to_str().unwrap()}),
        &mut state_a,
    )
    .unwrap();
    let _ = handle_tool_call(
        "session_continuity",
        json!({"operation": "save", "project_path": temp_dir.to_str().unwrap()}),
        &mut state_a,
    )
    .unwrap();
    let db = crate::context::db::CodeGraphDb::open_for_project(&temp_dir).unwrap();
    let count: i64 = db
        .conn
        .query_row(
            "SELECT co_change_count FROM co_change_edges WHERE file_a='src/a.rs' AND file_b='src/b.rs'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "Repeated save in same session must not double-count co-changes");

    // 3. Default clear removes only active session, preserving other sessions
    let mut state_b = ServerState::new();
    state_b.priority_gate_pass();
    state_b.save_to_dir(&temp_dir).unwrap();
    let id_a = state_a.session_id.clone();
    let id_b = state_b.session_id.clone();

    let _ = handle_tool_call(
        "session_continuity",
        json!({"operation": "clear", "project_path": temp_dir.to_str().unwrap()}),
        &mut state_a,
    )
    .unwrap();
    assert!(!temp_dir.join(format!(".agent-context/sessions/{}.json", id_a)).exists());
    assert!(temp_dir.join(format!(".agent-context/sessions/{}.json", id_b)).exists());

    // 4. Switch resets priority_gate_passed consistently with load
    state_a.priority_gate_pass();
    let _ = handle_tool_call(
        "session_continuity",
        json!({"operation": "switch", "session_id": id_b, "project_path": temp_dir.to_str().unwrap()}),
        &mut state_a,
    )
    .unwrap();
    assert!(!state_a.priority_gate_passed);

    // 5. clear with all=true wipes everything
    let _ = handle_tool_call(
        "session_continuity",
        json!({"operation": "clear", "all": true, "project_path": temp_dir.to_str().unwrap()}),
        &mut state_a,
    )
    .unwrap();
    assert!(!temp_dir.join(".agent-context/sessions").exists());

    let _ = std::fs::remove_dir_all(&temp_dir);
}
