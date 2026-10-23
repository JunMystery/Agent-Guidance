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
        assert!(state.edit_authorized);
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
}
