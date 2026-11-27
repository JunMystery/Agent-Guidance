    use super::*;

    #[test]
    fn test_cancellation_flag_lifecycle() {
        let mut state = ServerState::new();
        let flag = Arc::new(AtomicBool::new(false));
        state.set_cancellation(flag.clone());
        assert!(!state.is_cancelled());
        flag.store(true, Ordering::Relaxed);
        assert!(state.is_cancelled());
        state.clear_cancellation();
        assert!(!state.is_cancelled());
    }

    #[test]
    fn test_stage_transitions_and_circuit_breaker() {
        let mut state = ServerState::new();
        state.priority_gate_pass();
        assert_eq!(state.workflow_stage, "Context");
        assert!(!state.plan_approved);

        assert!(state.set_stage("Plan").is_ok());
        assert_eq!(state.workflow_stage, "Plan");
        assert!(!state.edit_authorized);

        // Transitioning to Build should fail if not approved
        assert!(state.set_stage("Build").is_err());

        // Process approval
        assert!(state.process_user_message("Looks good, proceed!"));
        assert!(state.plan_approved);

        // Now transition to Build succeeds
        assert!(state.set_stage("Build").is_ok());

        // Test Circuit breaker in Fix stage
        assert!(state.set_stage("Fix").is_ok());
        assert_eq!(state.fix_attempts, 1);
        assert!(state.set_stage("Fix").is_ok());
        assert_eq!(state.fix_attempts, 2);
        assert!(state.set_stage("Fix").is_ok());
        assert_eq!(state.fix_attempts, 3);

        // 4th Fix attempt triggers circuit breaker reset
        let res = state.set_stage("Fix");
        assert!(res.is_err());
        assert_eq!(state.workflow_stage, "Ask_Revise");
        assert!(!state.plan_approved);
        assert_eq!(state.fix_attempts, 0);
    }

    #[test]
    fn test_priority_gate_and_tool_categories() {
        // Ensure sentinel file from previous runs is cleaned up for test isolation
        let path = ServerState::priority_gate_path();
        if path.exists() {
            let _ = fs::remove_file(&path);
        }

        let mut state = ServerState::new();

        // 1. Gated tool fails initially with PRIORITY_REQUIRED
        let err = state
            .can_call_tool("guidance", &serde_json::json!({}))
            .unwrap_err();
        assert!(err.contains("PRIORITY_REQUIRED"));
        assert!(err.contains("Call task_pipeline first"));
        assert!(!err.contains("agent-guidance-mcp_task_pipeline"));

        // 2. Whitelisted & Not Gated tools succeed without priority gate unlock
        assert!(
            state
                .can_call_tool("workflow_gate", &serde_json::json!({}))
                .is_ok()
        );
        assert!(
            state
                .can_call_tool(
                    "session_continuity",
                    &serde_json::json!({"operation": "load"})
                )
                .is_ok()
        );

        // 3. Calling task_pipeline unlocks priority gate and advances stage to Plan
        assert!(
            state
                .can_call_tool("task_pipeline", &serde_json::json!({}))
                .is_ok()
        );
        assert!(state.priority_gate_passed);
        assert_eq!(state.workflow_stage, "Plan");

        // 4. Now gated tool passes priority check and stage check in Plan stage
        assert!(
            state
                .can_call_tool("workflow_gate", &serde_json::json!({}))
                .is_ok()
        );

        // Clean up sentinel file created by task_pipeline
        if path.exists() {
            let _ = fs::remove_file(&path);
        }
    }

    #[test]
    fn test_parse_file_uri_cross_platform() {
        assert_eq!(
            parse_file_uri("file:///C:/Users/test/project"),
            if cfg!(windows) {
                "C:\\Users\\test\\project"
            } else {
                "/C:/Users/test/project"
            }
        );
        assert_eq!(
            parse_file_uri("file:///e:/Github/Agent-Guidance-Rust"),
            if cfg!(windows) {
                "E:\\Github\\Agent-Guidance-Rust"
            } else {
                "/e:/Github/Agent-Guidance-Rust"
            }
        );
        assert_eq!(
            parse_file_uri("file:///home/user/project%20name"),
            if cfg!(windows) {
                "\\home\\user\\project name"
            } else {
                "/home/user/project name"
            }
        );
    }

    #[test]
    fn test_vietnamese_approval_keywords() {
        let mut state = ServerState::new();
        assert!(!state.plan_approved);

        assert!(state.process_user_message("chốt triển khai đi bro"));
        assert!(state.plan_approved);

        state.plan_approved = false;
        assert!(state.process_user_message("đồng ý duyệt"));
        assert!(state.plan_approved);
    }

    #[test]
    fn test_task_pipeline_resets_plan_approval_for_new_task() {
        let mut state = ServerState::new();
        state.workflow_stage = "Build".to_string();
        state.plan_approved = true;
        state.edit_authorized = true;

        // Simulate reset logic executed when task_pipeline phase="plan" is invoked
        state.workflow_stage = "Plan".to_string();
        state.plan_approved = false;
        state.edit_authorized = false;
        state.verification_passed = false;
        state.verification_command = None;
        state.expected_output_keyword = None;

        assert_eq!(state.workflow_stage, "Plan");
        assert!(!state.plan_approved);
        assert!(!state.edit_authorized);
    }

    #[test]
    fn test_multi_session_isolation() {
        let temp_dir =
            std::env::temp_dir().join(format!("multi_session_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&temp_dir);

        let mut s1 = ServerState::with_client_name(Some("antigravity".to_string()));
        s1.workflow_stage = "Build".to_string();
        s1.plan_approved = true;
        assert!(s1.save_to_dir(&temp_dir).is_ok());

        let mut s2 = ServerState::with_client_name(Some("cursor".to_string()));
        s2.workflow_stage = "Plan".to_string();
        s2.plan_approved = false;
        assert!(s2.save_to_dir(&temp_dir).is_ok());

        // Both session files exist independently in sessions/
        let sessions_dir = temp_dir.join(".agent-context").join("sessions");
        assert!(
            sessions_dir
                .join(format!("{}.json", s1.session_id))
                .exists()
        );
        assert!(
            sessions_dir
                .join(format!("{}.json", s2.session_id))
                .exists()
        );
        assert_ne!(s1.session_id, s2.session_id);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_session_gc_cleanup() {
        let temp_dir = std::env::temp_dir().join(format!("gc_cleanup_test_{}", std::process::id()));
        let sessions_dir = temp_dir.join(".agent-context").join("sessions");
        let _ = std::fs::create_dir_all(&sessions_dir);

        // Create 105 mock session files
        for i in 0..105 {
            let f = sessions_dir.join(format!("session_mock_{}.json", i));
            let _ = std::fs::write(&f, r#"{"session_id":"mock"}"#);
        }

        ServerState::cleanup_stale_sessions(&temp_dir);

        let count = std::fs::read_dir(&sessions_dir).unwrap().count();
        assert!(count <= 100);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_workflow_gate_review_stage_alias() {
        let mut state = ServerState::new();
        state.approve_plan();
        assert_eq!(state.set_stage("Build").unwrap(), "Build");
        assert_eq!(state.set_stage("Review").unwrap(), "Proposal");
        assert_eq!(state.workflow_stage, "Proposal");

        let mut state2 = ServerState::new();
        assert_eq!(state2.set_stage("review").unwrap(), "Proposal");
        assert_eq!(state2.workflow_stage, "Proposal");
    }

    #[test]
    fn test_auto_checkpoint_atomic_write() {
        let temp_dir = std::env::temp_dir().join(format!("auto_cp_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);

        let mut state = ServerState::new();
        state.approve_plan();
        state.set_stage("Build").unwrap();
        state.active_architecture_pattern = Some("Layered_Architecture".to_string());

        assert!(state.auto_checkpoint(&temp_dir).is_ok());

        let file_path = temp_dir
            .join(".agent-context")
            .join("sessions")
            .join(format!("{}.json", state.session_id));
        assert!(file_path.exists(), "Checkpoint file should exist");

        let loaded = ServerState::load_from_dir(&temp_dir).unwrap();
        assert_eq!(loaded.session_id, state.session_id);
        assert_eq!(loaded.workflow_stage, "Build");
        assert_eq!(
            loaded.active_architecture_pattern.as_deref(),
            Some("Layered_Architecture")
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_list_sessions_and_load_by_id() {
        let temp_dir = std::env::temp_dir().join(format!("list_sessions_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);

        let mut s1 = ServerState::with_client_name(Some("antigravity".to_string()));
        s1.workflow_stage = "Build".to_string();
        s1.record_modified_file("src/main.rs");
        assert!(s1.save_to_dir(&temp_dir).is_ok());

        let mut s2 = ServerState::with_client_name(Some("cursor".to_string()));
        s2.workflow_stage = "Plan".to_string();
        assert!(s2.save_to_dir(&temp_dir).is_ok());

        let sessions = ServerState::list_sessions(&temp_dir);
        assert_eq!(sessions.len(), 2);

        let loaded_s1 = ServerState::load_session_by_id(&temp_dir, &s1.session_id).unwrap();
        assert_eq!(loaded_s1.session_id, s1.session_id);
        assert_eq!(loaded_s1.workflow_stage, "Build");
        assert_eq!(loaded_s1.modified_files, vec!["src/main.rs"]);

        assert!(ServerState::load_session_by_id(&temp_dir, "non_existent_session").is_err());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_approval_message_word_boundary_prevents_substring_bypass() {
        let mut state = ServerState::new();
        assert!(!state.process_user_message("Fix auth token refresh and hook startup"));
        assert!(!state.plan_approved);

        assert!(!state.process_user_message("Execute migration script in test sandbox"));
        assert!(!state.plan_approved);

        assert!(!state.process_user_message("No, cancel and reject this plan"));
        assert!(!state.plan_approved);

        assert!(state.process_user_message("OK, go ahead"));
        assert!(state.plan_approved);
    }

    #[test]
    fn test_with_client_name_generates_unique_session_ids() {
        let s1 = ServerState::with_client_name(Some("antigravity".to_string()));
        let s2 = ServerState::with_client_name(Some("antigravity".to_string()));
        assert_ne!(s1.session_id, s2.session_id);
    }

    #[test]
    fn test_update_project_path_resets_cross_workspace_state() {
        let base = std::env::temp_dir().join(format!("ws_reset_test_{}", std::process::id()));
        let repo_a = base.join("repo_a");
        let repo_b = base.join("repo_b");
        let _ = std::fs::create_dir_all(&repo_a);
        let _ = std::fs::create_dir_all(&repo_b);

        let mut state = ServerState::new();
        state.update_project_path(&repo_a);
        state.plan_approved = true;
        state.edit_authorized = true;
        state.workflow_stage = "Build".to_string();
        state.record_modified_file("src/a.rs");
        state.push_recent_task("Task in repo A");

        state.update_project_path(&repo_b);
        assert!(!state.plan_approved);
        assert!(!state.edit_authorized);
        assert_eq!(state.workflow_stage, "Context");
        assert!(state.modified_files.is_empty());
        assert!(state.recent_tasks.is_empty());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn test_priority_gate_rejects_expired_and_cross_project_sentinel() {
        let path = ServerState::priority_gate_path();
        let _ = fs::remove_file(&path);

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Expired sentinel should be rejected
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let expired = serde_json::json!({
            "status": "PASSED",
            "timestamp": now.saturating_sub(super::types::SESSION_STALE_TIMEOUT_SECS + 60),
            "project_path": "d:/repo_a"
        });
        fs::write(&path, expired.to_string()).unwrap();
        let mut s1 = ServerState::new();
        assert!(s1.priority_gate_check().is_err());

        // Fresh sentinel from different project should be rejected
        let cross_proj = serde_json::json!({
            "status": "PASSED",
            "timestamp": now,
            "project_path": "d:/repo_a"
        });
        fs::write(&path, cross_proj.to_string()).unwrap();
        let mut s2 = ServerState::new();
        s2.project_path = Some("d:/repo_b".to_string());
        assert!(s2.priority_gate_check().is_err());

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_build_unapproved_exempts_gate_and_continuity_and_requires_priority_gate() {
        let path = ServerState::priority_gate_path();
        let _ = fs::remove_file(&path);

        let mut no_prio = ServerState::new();
        no_prio.project_path = Some("d:/repo_prio".to_string());
        for args in [
            serde_json::json!({"action": "approve_plan", "user_confirmed": true}),
            serde_json::json!({"action": "approve", "user_confirmed": true}),
            serde_json::json!({"action": "authorize_edit", "relative_path": "src/main.rs"}),
            serde_json::json!({"action": "set_stage", "target_stage": "Build"}),
            serde_json::json!({"action": "set_stage", "target_stage": "Fix"}),
            serde_json::json!({"action": "advance"}),
            serde_json::json!({"action": "advance_stage", "stage": "Fix"}),
        ] {
            let err = no_prio.can_call_tool("workflow_gate", &args).unwrap_err();
            assert!(err.contains("PRIORITY_REQUIRED"), "Expected priority block for {:?}", args);
        }
        assert!(no_prio.can_call_tool("workflow_gate", &serde_json::json!({"action": "check"})).is_ok());
        assert!(no_prio.can_call_tool("workflow_gate", &serde_json::json!({"action": "status"})).is_ok());
        assert!(no_prio.can_call_tool("workflow_gate", &serde_json::json!({"action": "set_stage", "target_stage": "Plan"})).is_ok());

        let mut restored = ServerState::new();
        restored.priority_gate_pass();
        restored.workflow_stage = "Build".to_string();
        restored.plan_approved = false;

        for action in [
            "check",
            "status",
            "approve_plan",
            "approve",
            "set_stage",
            "advance",
            "rollback",
            "set_architecture",
            "pass_verification",
            "invalidate",
            "sync_file",
        ] {
            assert!(
                restored
                    .can_call_tool("workflow_gate", &serde_json::json!({"action": action}))
                    .is_ok(),
                "Action '{}' should not deadlock in Build && !plan_approved",
                action
            );
        }
        assert!(restored.can_call_tool("session_continuity", &serde_json::json!({"operation": "load"})).is_ok());
        assert!(restored.can_call_tool("workflow_gate", &serde_json::json!({"action": "authorize_edit"})).is_err());
        assert!(restored.can_call_tool("guidance", &serde_json::json!({"operation": "precode"})).is_err());

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn test_fix_circuit_breaker_persists_across_test_recheck_loops() {
        let mut state = ServerState::new();
        assert!(state.set_stage("Fix").is_err(), "Fix requires plan_approved=true");

        state.approve_plan();
        state.set_stage("Build").unwrap();
        assert_eq!(state.set_stage("Fix").unwrap(), "Fix");
        assert_eq!(state.fix_attempts, 1);
        assert_eq!(state.set_stage("Test_Recheck").unwrap(), "Test_Recheck");
        assert_eq!(state.fix_attempts, 1, "Test_Recheck must not reset fix_attempts");

        assert_eq!(state.set_stage("Fix").unwrap(), "Fix");
        assert_eq!(state.fix_attempts, 2);
        assert_eq!(state.set_stage("Test_Recheck").unwrap(), "Test_Recheck");
        assert_eq!(state.set_stage("Fix").unwrap(), "Fix");
        assert_eq!(state.fix_attempts, 3);
        assert_eq!(state.set_stage("Test_Recheck").unwrap(), "Test_Recheck");

        let err = state.set_stage("Fix").unwrap_err();
        assert!(err.contains("Circuit breaker"));
        assert_eq!(state.workflow_stage, "Ask_Revise");
        assert!(!state.plan_approved);
        assert!(!state.edit_authorized);
        assert_eq!(state.fix_attempts, 0);
    }

    #[test]
    fn test_sentinel_scoped_to_session_and_non_null_project_and_corrupt_legacy_fallback() {
        let path = ServerState::priority_gate_path();
        let _ = fs::remove_file(&path);
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let dir = std::env::temp_dir().join(format!("ag_sentinel_scope_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".agent-context")).unwrap();

        // Null project_path must be rejected even with matching session_id
        let mut s = ServerState::new();
        s.project_path = Some(dir.to_string_lossy().to_string());
        fs::write(
            &path,
            serde_json::json!({"status": "PASSED", "timestamp": now, "session_id": s.session_id, "project_path": serde_json::Value::Null}).to_string(),
        )
        .unwrap();
        assert!(s.priority_gate_check().is_err());

        // Mismatched session_id must be rejected even with matching project_path
        fs::write(
            &path,
            serde_json::json!({"status": "PASSED", "timestamp": now, "session_id": "other_session", "project_path": dir.to_string_lossy()}).to_string(),
        )
        .unwrap();
        assert!(s.priority_gate_check().is_err());

        // Matching session_id and project_path succeeds
        fs::write(
            &path,
            serde_json::json!({"status": "PASSED", "timestamp": now, "session_id": s.session_id, "project_path": dir.to_string_lossy()}).to_string(),
        )
        .unwrap();
        assert!(s.priority_gate_check().is_ok());

        // Corrupt legacy session.json falls back cleanly to fresh ServerState
        fs::write(dir.join(".agent-context").join("session.json"), "{broken json").unwrap();
        let loaded = ServerState::load_from_dir(&dir).unwrap();
        assert_eq!(loaded.workflow_stage, "Context");

        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir_all(&dir);
    }
