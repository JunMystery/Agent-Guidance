    use super::*;
    use crate::mcp::state::ServerState;

    #[test]
    fn test_router_resources_list_and_read() {
        let mut state = ServerState::new();

        // 1. Test resources/list
        let list_res = handle_request("resources/list", None, &mut state);
        assert!(list_res.is_ok());
        let list_val = list_res.unwrap();
        let resources = list_val["resources"].as_array().unwrap();
        assert!(resources.len() >= 5);

        // 2. Test resources/read for standards://version
        let read_params = json!({"uri": "standards://version"});
        let read_res = handle_request("resources/read", Some(read_params), &mut state);
        assert!(read_res.is_ok());
        let read_val = read_res.unwrap();
        let contents = &read_val["contents"][0];
        assert_eq!(contents["mimeType"], "application/json");
        assert!(
            contents["text"]
                .as_str()
                .unwrap()
                .contains("Agent Guidance MCP Rust")
        );

        // 3. Test resources/read for agent-guidance://system/priority and legacy alias
        let read_params = json!({"uri": "agent-guidance://system/priority"});
        let read_res = handle_request("resources/read", Some(read_params), &mut state);
        assert!(read_res.is_ok());
        let read_val = read_res.unwrap();
        let contents = &read_val["contents"][0];
        assert_eq!(contents["mimeType"], "text/markdown");
        assert!(
            contents["text"]
                .as_str()
                .unwrap()
                .contains("Priority Gate Instructions")
        );

        let legacy_res = handle_request(
            "resources/read",
            Some(json!({"uri": "agent-guidance-mcp://system/priority"})),
            &mut state,
        );
        assert!(legacy_res.is_ok());

        let gate_res = handle_request(
            "resources/read",
            Some(json!({"uri": "agent-guidance://system/gate"})),
            &mut state,
        );
        assert!(gate_res.is_ok());
    }

    #[test]
    fn test_all_tool_array_schemas_have_items() {
        let mut state = ServerState::new();
        let list_res = handle_request("tools/list", None, &mut state);
        assert!(list_res.is_ok());
        let list_val = list_res.unwrap();
        let tools = list_val["tools"].as_array().expect("tools array");

        for tool in tools {
            let tool_name = tool["name"].as_str().unwrap_or("unknown");
            let props = tool["inputSchema"]["properties"]
                .as_object()
                .expect("properties object");

            for (prop_name, prop_val) in props {
                if prop_val.get("type").and_then(|t| t.as_str()) == Some("array") {
                    assert!(
                        prop_val.get("items").is_some(),
                        "Tool '{}' parameter '{}' has type 'array' but is missing 'items'",
                        tool_name,
                        prop_name
                    );
                }
            }
        }
    }

    #[test]
    fn test_is_read_only_request_protects_stateful_ops() {
        assert!(!is_read_only_request("initialize", &None));
        assert!(!is_read_only_request(
            "tools/call",
            &Some(json!({ "name": "session_continuity", "arguments": { "operation": "load" } }))
        ));
        assert!(!is_read_only_request(
            "tools/call",
            &Some(json!({ "name": "project_context", "arguments": { "operation": "read", "authorize": true } }))
        ));
        assert!(!is_read_only_request(
            "tools/call",
            &Some(json!({ "name": "project_context", "arguments": { "operation": "read_and_authorize" } }))
        ));
        assert!(!is_read_only_request(
            "tools/call",
            &Some(json!({ "name": "project_context", "arguments": { "operation": "read", "conversation_id": "conv-2" } }))
        ));
        assert!(is_read_only_request(
            "tools/call",
            &Some(json!({ "name": "project_context", "arguments": { "operation": "read" } }))
        ));
        assert!(is_read_only_request(
            "tools/call",
            &Some(json!({ "name": "session_continuity", "arguments": { "operation": "list" } }))
        ));
    }

    #[test]
    fn test_tools_call_resets_state_on_conversation_switch() {
        let mut state = ServerState::new();
        state.priority_gate_pass();
        state.session_id = "conv-1".to_string();
        state.workflow_stage = "Build".to_string();
        state.plan_approved = true;
        state.edit_authorized = true;
        state.record_modified_file("src/old_repo.rs");
        state.push_recent_task("Old task");

        let res = handle_request(
            "tools/call",
            Some(json!({
                "name": "workflow_gate",
                "arguments": { "action": "status", "conversation_id": "conv-2" }
            })),
            &mut state,
        );
        assert!(res.is_ok());
        assert_eq!(state.session_id, "conv-2");
        assert_eq!(state.workflow_stage, "Context");
        assert!(!state.plan_approved);
        assert!(!state.edit_authorized);
        assert!(state.modified_files.is_empty());
        assert!(state.recent_tasks.is_empty());
    }

    #[test]
    fn test_tools_call_rejects_path_traversal_session_id_and_pre_authorize_readonly() {
        let mut state = ServerState::new();
        let err = handle_request(
            "tools/call",
            Some(json!({
                "name": "workflow_gate",
                "arguments": { "action": "status", "conversation_id": "../../outside" }
            })),
            &mut state,
        )
        .unwrap_err();
        assert_eq!(err.0, -32602);
        assert!(err.1.contains("Invalid session_id"));

        assert!(!is_read_only_request(
            "tools/call",
            &Some(json!({ "name": "project_context", "arguments": { "operation": "subgraph_bundle", "pre_authorize": true } }))
        ));
        assert!(!is_read_only_request(
            "tools/call",
            &Some(json!({ "name": "project_context", "arguments": { "operation": "architecture" } }))
        ));
    }

    #[test]
    fn test_pass3_router_and_gate_hardening() {
        let mut state = ServerState::new();
        state.priority_gate_pass();
        state.pending_skill_proposals = vec![("agent-guidance".to_string(), "skills/agent-guidance/SKILL.md".to_string(), 1.0)];
        let err = state.can_call_tool("workflow_gate", &json!({"action": "set_stage", "target_stage": "Build", "user_confirmed": true})).unwrap_err();
        assert!(err.contains("SKILL_PROPOSAL_PENDING"));

        state.pending_skill_proposals.clear();
        state.workflow_stage = "Build".to_string();
        state.plan_approved = true;
        state.edit_authorized = false;
        let res = resources::handle_read(Some(json!({"uri": "agent-guidance://system/edit-allowed"})), &state).unwrap();
        let body: serde_json::Value = serde_json::from_str(res["contents"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(body["edit_allowed"], json!(false));

        let decoded = crate::mcp::state::uri::parse_file_uri("file:///d%3A/My%20Repo");
        assert!(decoded.contains("My Repo"));

        let get_res = handle_tool_call("guidance", json!({"operation": "get", "topic": "Cargo.toml", "project_path": "."}), &mut state).unwrap();
        assert!(get_res["content"][0]["text"].as_str().unwrap().contains("Skill asset not found"));
    }
