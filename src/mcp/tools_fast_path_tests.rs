#[cfg(test)]
mod tests {
    use std::fs;
    use serde_json::json;
    use crate::mcp::state::ServerState;
    use crate::mcp::tools::handle_tool_call;

    #[test]
    fn test_task_pipeline_fast_path_keyword_detection() {
        let temp_dir = std::env::temp_dir().join(format!("ag_fast_kw_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);

        let mut state = ServerState::new();
        state.workspace_roots = vec![temp_dir.to_string_lossy().to_string()];

        let res = handle_tool_call(
            "task_pipeline",
            json!({
                "task": "Fix typo in README.md",
                "project_path": temp_dir.to_string_lossy().to_string()
            }),
            &mut state,
        );

        assert!(res.is_ok());
        let text = res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
        assert!(text.contains("Minor Task Fast-Path Exemption"), "Output should activate fast-path: {}", text);
        assert_eq!(state.workflow_stage, "Build");
        assert!(state.plan_approved);
        assert!(state.is_fast_path);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_task_pipeline_fast_path_explicit_flag() {
        let temp_dir = std::env::temp_dir().join(format!("ag_fast_flag_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);

        let mut state = ServerState::new();
        state.workspace_roots = vec![temp_dir.to_string_lossy().to_string()];

        let res = handle_tool_call(
            "task_pipeline",
            json!({
                "task": "Update token header key",
                "is_minor": true,
                "project_path": temp_dir.to_string_lossy().to_string()
            }),
            &mut state,
        );

        assert!(res.is_ok());
        let text = res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
        assert!(text.contains("Minor Task Fast-Path Exemption"));
        assert_eq!(state.workflow_stage, "Build");
        assert!(state.plan_approved);
        assert!(state.is_fast_path);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_task_pipeline_regular_task_not_fast_path() {
        let temp_dir = std::env::temp_dir().join(format!("ag_regular_task_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);

        let mut state = ServerState::new();
        state.workspace_roots = vec![temp_dir.to_string_lossy().to_string()];

        let res = handle_tool_call(
            "task_pipeline",
            json!({
                "task": "Plan architecture for new payment gateway",
                "phase": "plan",
                "project_path": temp_dir.to_string_lossy().to_string()
            }),
            &mut state,
        );

        assert!(res.is_ok());
        let text = res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
        assert!(!text.contains("Fast-Path Exemption"), "Complex task must not be fast-path: {}", text);
        assert_eq!(state.workflow_stage, "Plan");
        assert!(!state.plan_approved);
        assert!(!state.is_fast_path);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_fast_path_boundary_guard_blocks_multi_code_files() {
        let temp_dir = std::env::temp_dir().join(format!("ag_fast_guard_{}", std::process::id()));
        let _ = fs::create_dir_all(temp_dir.join("src"));
        fs::write(temp_dir.join("src/first.rs"), "pub fn first() {}\n").unwrap();
        fs::write(temp_dir.join("src/second.rs"), "pub fn second() {}\n").unwrap();

        let mut state = ServerState::new();
        state.workspace_roots = vec![temp_dir.to_string_lossy().to_string()];
        state.is_fast_path = true;
        state.workflow_stage = "Build".to_string();
        state.plan_approved = true;

        // First code file succeeds
        let res1 = handle_tool_call(
            "workflow_gate",
            json!({
                "action": "authorize_edit",
                "relative_path": "src/first.rs",
                "project_path": temp_dir.to_string_lossy().to_string()
            }),
            &mut state,
        );
        assert!(res1.is_ok());
        let text1 = res1.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
        assert!(text1.contains("Status: PASSED"), "First code file should pass: {}", text1);

        // Second code file in fast-path must be blocked by Boundary Guard
        let res2 = handle_tool_call(
            "workflow_gate",
            json!({
                "action": "authorize_edit",
                "relative_path": "src/second.rs",
                "project_path": temp_dir.to_string_lossy().to_string()
            }),
            &mut state,
        );
        assert!(res2.is_ok());
        let text2 = res2.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
        assert!(text2.contains("FAST_PATH_BOUNDARY_EXCEEDED"), "Second code file must be blocked: {}", text2);
        assert_eq!(state.workflow_stage, "Plan", "Stage must be reset to Plan");
        assert!(!state.plan_approved, "Plan approved must be revoked");
        assert!(!state.is_fast_path, "Fast path must be cleared");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_fast_path_boundary_guard_allows_up_to_3_doc_files() {
        let temp_dir = std::env::temp_dir().join(format!("ag_fast_docs_{}", std::process::id()));
        let _ = fs::create_dir_all(temp_dir.join("docs"));
        fs::write(temp_dir.join("docs/a.md"), "# A\n").unwrap();
        fs::write(temp_dir.join("docs/b.md"), "# B\n").unwrap();
        fs::write(temp_dir.join("README.md"), "# Readme\n").unwrap();
        fs::write(temp_dir.join("CHANGELOG.md"), "# Changelog\n").unwrap();

        let mut state = ServerState::new();
        state.workspace_roots = vec![temp_dir.to_string_lossy().to_string()];
        state.is_fast_path = true;
        state.workflow_stage = "Build".to_string();
        state.plan_approved = true;

        // 3 doc files pass
        for doc in &["docs/a.md", "docs/b.md", "README.md"] {
            let res = handle_tool_call(
                "workflow_gate",
                json!({
                    "action": "authorize_edit",
                    "relative_path": doc,
                    "project_path": temp_dir.to_string_lossy().to_string()
                }),
                &mut state,
            );
            assert!(res.is_ok());
            let text = res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
            assert!(text.contains("Status: PASSED"), "Doc file should pass: {}", text);
        }

        // 4th doc file is blocked
        let res4 = handle_tool_call(
            "workflow_gate",
            json!({
                "action": "authorize_edit",
                "relative_path": "CHANGELOG.md",
                "project_path": temp_dir.to_string_lossy().to_string()
            }),
            &mut state,
        );
        assert!(res4.is_ok());
        let text4 = res4.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
        assert!(text4.contains("FAST_PATH_BOUNDARY_EXCEEDED"), "4th doc file must trigger boundary guard: {}", text4);

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
