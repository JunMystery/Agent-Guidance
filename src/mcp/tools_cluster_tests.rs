#[cfg(test)]
mod tests {
    use std::fs;
    use serde_json::json;
    use crate::mcp::state::ServerState;
    use crate::mcp::tools::handle_tool_call;

    #[test]
    fn test_parse_target_spec_variations() {
        use crate::mcp::tools::helpers_spec::TargetPathSpec;

        // 1. Plain relative path
        let s1 = TargetPathSpec::parse("src/lib.rs");
        assert_eq!(s1.clean_path, "src/lib.rs");
        assert_eq!(s1.target_symbol, None);
        assert_eq!(s1.line_range, None);

        // 2. Symbol fragment
        let s2 = TargetPathSpec::parse("src/service.rs#execute");
        assert_eq!(s2.clean_path, "src/service.rs");
        assert_eq!(s2.target_symbol.as_deref(), Some("execute"));
        assert_eq!(s2.line_range, None);

        // 3. Line range
        let s3 = TargetPathSpec::parse("src/service.rs:15-40");
        assert_eq!(s3.clean_path, "src/service.rs");
        assert_eq!(s3.target_symbol, None);
        assert_eq!(s3.line_range, Some((15, 40)));

        // 4. Single line
        let s4 = TargetPathSpec::parse("src/service.rs:25");
        assert_eq!(s4.clean_path, "src/service.rs");
        assert_eq!(s4.line_range, Some((25, 25)));

        // 5. Windows drive letter with symbol
        let s5 = TargetPathSpec::parse(r"C:\workspace\file.rs#run");
        assert_eq!(s5.clean_path, "C:/workspace/file.rs");
        assert_eq!(s5.target_symbol.as_deref(), Some("run"));
        assert_eq!(s5.line_range, None);

        // 6. Windows drive letter with range
        let s6 = TargetPathSpec::parse(r"D:\repo\file.rs:10-30");
        assert_eq!(s6.clean_path, "D:/repo/file.rs");
        assert_eq!(s6.line_range, Some((10, 30)));
    }

    #[test]
    fn test_clustered_read_with_symbol_and_lines() {
        let temp_dir = std::env::temp_dir().join(format!("ag_cluster_test_{}", std::process::id()));
        let _ = fs::create_dir_all(temp_dir.join("src"));

        let code_file = temp_dir.join("src/math.rs");
        let code_content = "\n\npub fn helper() -> i32 {\n    10\n}\n\npub fn calculate() -> i32 {\n    let a = 1;\n    let b = 2;\n    a + b\n}\n";
        fs::write(&code_file, code_content).unwrap();

        let cfg_file = temp_dir.join("config.yaml");
        let cfg_content = "line1: a\nline2: b\nline3: c\nline4: d\nline5: target\nline6: target\nline7: g\n";
        fs::write(&cfg_file, cfg_content).unwrap();

        let mut state = ServerState::new();
        state.workspace_roots = vec![temp_dir.to_string_lossy().to_string()];

        let res = handle_tool_call(
            "project_context",
            json!({
                "operation": "read",
                "relative_paths": [
                    "src/math.rs#calculate",
                    "config.yaml:5-6"
                ]
            }),
            &mut state,
        );

        assert!(res.is_ok(), "Clustered read should succeed");
        let text = res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();

        assert!(text.contains("Symbol: 'calculate'"), "Output should contain sliced calculate symbol: {}", text);
        assert!(text.contains("pub fn calculate"), "Output should contain calculate function body: {}", text);
        assert!(!text.contains("pub fn helper"), "Output should not contain unrelated helper function: {}", text);
        assert!(text.contains("Lines 5-6 of 7"), "Output should slice lines 5-6: {}", text);
        assert!(text.contains("line5: target"), "Output should contain line 5 content: {}", text);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_clustered_read_new_file_and_authorize() {
        let temp_dir = std::env::temp_dir().join(format!("ag_cluster_auth_{}", std::process::id()));
        let _ = fs::create_dir_all(temp_dir.join("src"));

        let code_file = temp_dir.join("src/lib.rs");
        fs::write(&code_file, "pub fn lib() {}\n").unwrap();

        let mut state = ServerState::new();
        state.workspace_roots = vec![temp_dir.to_string_lossy().to_string()];
        state.plan_approved = true;
        state.workflow_stage = "Plan".to_string(); // Will auto-advance to Build

        let res = handle_tool_call(
            "project_context",
            json!({
                "operation": "read",
                "relative_paths": [
                    "src/lib.rs",
                    "src/new_service.rs"
                ],
                "authorize": true
            }),
            &mut state,
        );

        assert!(res.is_ok(), "Clustered read with authorize: true should succeed");
        let text = res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();

        assert!(text.contains("[NEW FILE - Pending Creation]"), "Non-existent file should be marked NEW FILE: {}", text);
        assert_eq!(state.workflow_stage, "Build", "Stage should auto-advance to Build on authorized read");
        assert!(text.contains("Direct Edit Authorization Report"), "Report should be included: {}", text);
        assert!(state.edit_authorized, "Edit authorization should be granted");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_single_file_read_with_symbol_spec() {
        let temp_dir = std::env::temp_dir().join(format!("ag_single_sym_{}", std::process::id()));
        let _ = fs::create_dir_all(temp_dir.join("src"));

        let code_file = temp_dir.join("src/lib.rs");
        let code_content = "pub fn foo() -> i32 { 1 }\npub fn bar() -> i32 { 2 }\n";
        fs::write(&code_file, code_content).unwrap();

        let mut state = ServerState::new();
        state.workspace_roots = vec![temp_dir.to_string_lossy().to_string()];

        let res = handle_tool_call(
            "project_context",
            json!({
                "operation": "read",
                "relative_path": "src/lib.rs#bar"
            }),
            &mut state,
        );

        assert!(res.is_ok());
        let text = res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
        assert!(text.contains("Target Symbol Extracted: 'bar'"));
        assert!(text.contains("pub fn bar"));
        assert!(!text.contains("pub fn foo"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_deferred_pre_authorization_queue_and_approve_plan() {
        let temp_dir = std::env::temp_dir().join(format!("ag_defer_auth_{}", std::process::id()));
        let _ = fs::create_dir_all(temp_dir.join("src"));
        fs::write(temp_dir.join("src/lib.rs"), "pub fn lib() {}\n").unwrap();

        let mut state = ServerState::new();
        state.workspace_roots = vec![temp_dir.to_string_lossy().to_string()];
        state.workflow_stage = "Plan".to_string();
        state.plan_approved = false;

        // 1. Clustered read in Plan stage with authorize: true enqueues deferred paths
        let res = handle_tool_call(
            "project_context",
            json!({
                "operation": "read",
                "relative_paths": [
                    "src/lib.rs",
                    "src/new_mod.rs"
                ],
                "authorize": true
            }),
            &mut state,
        );

        assert!(res.is_ok());
        let text = res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
        assert!(text.contains("Deferred Pre-Authorization Queued"), "Should report queued deferred authorization: {}", text);
        assert_eq!(state.deferred_authorized_paths.len(), 2);
        assert_eq!(state.workflow_stage, "Plan");
        assert!(!state.plan_approved);

        // 2. Approve plan automatically drains deferred queue and pre-authorizes
        let res_approve = handle_tool_call(
            "workflow_gate",
            json!({
                "action": "approve_plan",
                "user_confirmed": true,
                "project_path": temp_dir.to_string_lossy().to_string()
            }),
            &mut state,
        );

        assert!(res_approve.is_ok());
        let approve_text = res_approve.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
        assert!(approve_text.contains("Batch Pre-Authorization: 2 file(s) processed"));
        assert!(state.deferred_authorized_paths.is_empty(), "Queue must be drained upon approval");
        assert_eq!(state.workflow_stage, "Build");
        assert!(state.plan_approved);

        // 3. Resetting stage to Plan clears any deferred paths
        state.deferred_authorized_paths.push("src/stale.rs".to_string());
        let _ = state.set_stage("Plan");
        assert!(state.deferred_authorized_paths.is_empty(), "Stage reset to Plan must auto-purge deferred queue");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_deferred_pre_authorization_warns_compound_plural_without_blocking() {
        let temp_dir = std::env::temp_dir().join(format!("ag_defer_warn_{}", std::process::id()));
        let _ = fs::create_dir_all(temp_dir.join("src"));

        let mut state = ServerState::new();
        state.workspace_roots = vec![temp_dir.to_string_lossy().to_string()];
        state.workflow_stage = "Plan".to_string();
        state.plan_approved = false;

        let res = handle_tool_call(
            "project_context",
            json!({
                "operation": "read",
                "relative_paths": [
                    "src/user_services.rs"
                ],
                "authorize": true
            }),
            &mut state,
        );

        assert!(res.is_ok());
        let text = res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
        assert!(text.contains("Deferred Pre-Authorization Queued"), "Compound plural must still queue: {}", text);
        assert!(text.contains("DOMAIN_COHESION_ADVISORY"));
        assert_eq!(state.deferred_authorized_paths.len(), 1);

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
