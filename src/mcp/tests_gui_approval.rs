#[cfg(test)]
mod tests {
    use crate::mcp::state::ServerState;
    use crate::mcp::tools;
    use serde_json::json;

    #[test]
    fn test_gui_approval_keywords_detected() {
        let mut state = ServerState::new();
        assert!(!state.plan_approved);

        let gui_msg = "Comments on artifact URI: file:///path/to/implementation_plan.md\n\nThe user has approved this document.";
        assert!(state.process_user_message(gui_msg));
        assert!(state.plan_approved);

        let mut state2 = ServerState::new();
        assert!(state2.process_user_message("Proceeded with Implementation Plan"));
        assert!(state2.plan_approved);

        let mut state3 = ServerState::new();
        assert!(state3.process_user_message("proceed with implementation plan"));
        assert!(state3.plan_approved);
    }

    #[test]
    fn test_task_pipeline_auto_advances_on_gui_approval() {
        let mut state = ServerState::new();
        state.workflow_stage = "Plan".to_string();
        state.plan_approved = false;

        let args = json!({
            "task": "Comments on artifact URI: file:///implementation_plan.md\n\nThe user has approved this document.",
            "phase": "plan"
        });

        let res = tools::handle_tool_call("task_pipeline", args, &mut state);
        assert!(res.is_ok());
        assert_eq!(state.workflow_stage, "Build");
        assert!(state.plan_approved);
        assert!(!state.edit_authorized);
    }

    #[test]
    fn test_workflow_gate_accepts_plan_approved_flag() {
        let mut state = ServerState::new();
        state.workflow_stage = "Plan".to_string();
        state.plan_approved = false;

        let args = json!({
            "action": "set_stage",
            "target_stage": "Build",
            "plan_approved": true
        });

        let res = tools::handle_tool_call("workflow_gate", args, &mut state);
        assert!(res.is_ok());
        assert_eq!(state.workflow_stage, "Build");
        assert!(state.plan_approved);
    }

    #[test]
    fn test_workflow_gate_advance_stage_alias() {
        let mut state = ServerState::new();
        state.workflow_stage = "Plan".to_string();
        state.plan_approved = false;

        let args = json!({
            "action": "advance_stage",
            "target_stage": "Build",
            "user_confirmed": true
        });

        let res = tools::handle_tool_call("workflow_gate", args, &mut state);
        assert!(res.is_ok());
        assert_eq!(state.workflow_stage, "Build");
        assert!(state.plan_approved);
    }

    #[test]
    fn test_workflow_gate_blocks_question_response_as_plan_approval() {
        let mut state = ServerState::new();
        state.workflow_stage = "Plan".to_string();
        state.plan_approved = false;

        let args = json!({
            "action": "approve_plan",
            "user_confirmed": true,
            "user_message": "A1: Thêm SAF filter\nA2: Đọc header CSV"
        });

        let res = tools::handle_tool_call("workflow_gate", args, &mut state);
        assert!(res.is_ok());
        let text = res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
        assert!(text.contains("QUESTION_RESPONSE_NOT_PLAN_APPROVAL"), "Got: {}", text);
        assert!(!state.plan_approved);
    }

    #[test]
    fn test_workflow_gate_blocks_question_choice_justification() {
        let mut state = ServerState::new();
        state.workflow_stage = "Plan".to_string();
        state.plan_approved = false;

        let args = json!({
            "action": "approve_plan",
            "user_confirmed": true,
            "justification": "Người dùng đã chọn phương án xử lý lỗi: SAF filter và CSV validation"
        });

        let res = tools::handle_tool_call("workflow_gate", args, &mut state);
        assert!(res.is_ok());
        let text = res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
        assert!(text.contains("QUESTION_CHOICE_NOT_PLAN_APPROVAL"), "Got: {}", text);
        assert!(!state.plan_approved);
    }

    #[test]
    fn test_workflow_gate_approve_plan_unconfirmed_error_message() {
        let mut state = ServerState::new();
        state.workflow_stage = "Plan".to_string();
        state.plan_approved = false;

        let args = json!({
            "action": "approve_plan",
            "user_confirmed": false
        });

        let res = tools::handle_tool_call("workflow_gate", args, &mut state);
        assert!(res.is_ok());
        let text = res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
        assert!(text.contains("USER_APPROVAL_REQUIRED"), "Got: {}", text);
        assert!(!text.contains("trigger `ask_question`"), "Should not encourage ask_question as alternative to approval: {}", text);
        assert!(!state.plan_approved);
    }

    #[test]
    fn test_workflow_gate_approve_plan_rejects_unconfirmed_negative_user_message() {
        let mut state = ServerState::new();
        state.workflow_stage = "Plan".to_string();
        state.plan_approved = false;

        let res = tools::handle_tool_call(
            "workflow_gate",
            json!({
                "action": "approve_plan",
                "user_confirmed": false,
                "user_message": "No, cancel this plan"
            }),
            &mut state,
        )
        .unwrap();
        let text = res["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("USER_APPROVAL_REQUIRED"), "Got: {}", text);
        assert!(!state.plan_approved);
    }

    #[test]
    fn test_workflow_gate_set_stage_and_advance_block_question_responses() {
        let mut state = ServerState::new();
        state.workflow_stage = "Plan".to_string();
        state.plan_approved = false;

        let res = tools::handle_tool_call(
            "workflow_gate",
            json!({
                "action": "advance",
                "target_stage": "Build",
                "user_confirmed": true,
                "user_message": "A1: Selected option 1"
            }),
            &mut state,
        )
        .unwrap();
        let text = res["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("QUESTION_RESPONSE_NOT_PLAN_APPROVAL"), "Got: {}", text);
        assert!(!state.plan_approved);
        assert_eq!(state.workflow_stage, "Plan");
    }

    #[test]
    fn test_task_pipeline_project_switch_with_build_phase_does_not_lock_context() {
        let base = std::env::temp_dir().join(format!("ag_pipe_switch_{}", std::process::id()));
        let repo_a = base.join("repo_a");
        let repo_b = base.join("repo_b");
        let _ = std::fs::create_dir_all(&repo_a);
        let _ = std::fs::create_dir_all(&repo_b);

        let mut state = ServerState::new();
        state.update_project_path(&repo_a);

        tools::handle_tool_call(
            "task_pipeline",
            json!({
                "task": "Implement auth handler",
                "phase": "build",
                "project_path": repo_b.to_str().unwrap()
            }),
            &mut state,
        )
        .unwrap();
        assert_ne!(state.workflow_stage, "Context");
        assert!(state.can_call_tool("project_context", &json!({"operation": "tree"})).is_ok());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn test_approve_plan_cross_workspace_block_preserves_state_and_deferred_queue() {
        let base = std::env::temp_dir().join(format!("ag_ap_pres_{}", std::process::id()));
        let repo_a = base.join("repo_a");
        let repo_b = base.join("repo_b");
        let _ = std::fs::create_dir_all(&repo_a);
        let _ = std::fs::create_dir_all(repo_b.join("src"));
        std::fs::write(repo_b.join("src/lib.rs"), "pub fn x() {}\n").unwrap();

        let mut state = ServerState::new();
        state.workspace_roots = vec![repo_a.to_string_lossy().to_string()];
        state.queue_deferred_authorization(&["src/lib.rs".to_string()]);

        let res = tools::handle_tool_call(
            "workflow_gate",
            json!({"action": "approve_plan", "user_confirmed": false, "user_message": "proceed", "project_path": repo_b.to_str().unwrap()}),
            &mut state,
        ).unwrap();
        assert!(res["content"][0]["text"].as_str().unwrap().contains("CROSS_WORKSPACE_WARNING"));
        assert!(!state.plan_approved);
        assert_eq!(state.deferred_authorized_paths, vec!["src/lib.rs"]);

        let adv = tools::handle_tool_call(
            "workflow_gate",
            json!({"action": "advance", "target_stage": "Build", "user_confirmed": true, "project_path": repo_b.to_str().unwrap(), "relative_paths": ["src/lib.rs"]}),
            &mut state,
        ).unwrap();
        assert!(adv["content"][0]["text"].as_str().unwrap().contains("CROSS_WORKSPACE_WARNING"));
        assert!(!state.plan_approved);
        assert_ne!(state.workflow_stage, "Build");

        state.is_fast_path = true;
        state.clear_deferred_authorized_paths();
        tools::handle_tool_call(
            "workflow_gate",
            json!({"action": "approve_plan", "user_confirmed": true, "project_path": repo_a.to_str().unwrap()}),
            &mut state,
        ).unwrap();
        assert!(state.plan_approved && !state.is_fast_path);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn test_short_new_task_in_plan_stage_and_priority_gate_and_session_id_guards() {
        let mut fresh = ServerState::new();
        tools::handle_tool_call("task_pipeline", json!({"task": "ok check logs", "phase": "plan"}), &mut fresh).unwrap();
        assert!(!fresh.plan_approved && fresh.workflow_stage == "Plan");

        let mut state = ServerState::new();
        state.workflow_stage = "Plan".to_string();
        state.plan_approved = false;

        tools::handle_tool_call(
            "task_pipeline",
            json!({ "task": "Triển khai API thanh toán", "phase": "plan" }),
            &mut state,
        )
        .unwrap();
        assert!(!state.plan_approved);
        assert_eq!(state.workflow_stage, "Plan");

        let path = ServerState::priority_gate_path();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(
            &path,
            json!({ "status": "PASSED", "timestamp": now, "project_path": "d:/other_repo" }).to_string(),
        )
        .unwrap();
        let mut unset = ServerState::new();
        assert!(unset.priority_gate_check().is_err());
        let _ = std::fs::remove_file(&path);

        assert!(ServerState::is_valid_session_id("conv-123_abc"));
        assert!(!ServerState::is_valid_session_id("../evil"));
        assert!(!ServerState::is_valid_session_id("a/b"));
        assert!(!ServerState::is_valid_session_id(""));
    }

    #[test]
    fn test_negation_anywhere_blocks_user_confirmed_and_ambiguous_phrases() {
        // Long message (>10 words) with negation mid-sentence must NOT approve
        let long_neg = "I read the whole plan carefully and please do not proceed with these edits right now";
        assert!(!ServerState::is_approval_message(long_neg));
        assert!(!ServerState::is_gui_or_short_plan_approval(long_neg));

        for neg in [
            "Khoan đã, ok để kiểm tra lại",
            "Đừng triển khai vội nhé",
            "Chưa đồng ý đâu",
            "Wait, ok let me check first",
            "I never approved this plan",
            "Reject this change, ok?",
            "Comments on artifact URI: file:///plan.md\n\nPlease fix section 2",
            "start the server",
            "exec query",
        ] {
            assert!(!ServerState::is_approval_message(neg), "Should reject: {}", neg);
            assert!(!ServerState::is_gui_or_short_plan_approval(neg), "GUI check should reject: {}", neg);
        }

        // user_confirmed=true must NOT override explicit negation in user_message
        let mut state = ServerState::new();
        state.workflow_stage = "Plan".to_string();
        let res = tools::handle_tool_call(
            "workflow_gate",
            json!({
                "action": "approve_plan",
                "user_confirmed": true,
                "user_message": "Khoan, đừng triển khai"
            }),
            &mut state,
        )
        .unwrap();
        assert!(res["content"][0]["text"].as_str().unwrap().contains("USER_APPROVAL_NEGATED"));
        assert!(!state.plan_approved);

        let adv = tools::handle_tool_call(
            "workflow_gate",
            json!({
                "action": "advance",
                "target_stage": "Build",
                "user_confirmed": true,
                "user_message": "Wait, do not proceed yet"
            }),
            &mut state,
        )
        .unwrap();
        assert!(adv["content"][0]["text"].as_str().unwrap().contains("USER_APPROVAL_NEGATED"));
        assert!(!state.plan_approved);
        assert_eq!(state.workflow_stage, "Plan");
    }
}
