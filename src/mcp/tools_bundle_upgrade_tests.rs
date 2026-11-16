#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use serde_json::json;

    use crate::context::db::CodeGraphDb;
    use crate::mcp::state::ServerState;
    use crate::mcp::tools::context_bundle::handle_subgraph_bundle;

    static UPGRADE_BUNDLE_TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TestProjectGuard {
        dir: std::path::PathBuf,
    }

    impl Drop for TestProjectGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    fn create_mock_project() -> (TestProjectGuard, CodeGraphDb) {
        let id = UPGRADE_BUNDLE_TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let temp_dir = std::env::temp_dir().join(format!("ag_bundle_upg_test_{}_{}", std::process::id(), id));
        let src_dir = temp_dir.join("src");
        let _ = fs::create_dir_all(&src_dir);

        let service_file = src_dir.join("service.rs");
        let handler_file = src_dir.join("handler.rs");
        let repo_file = src_dir.join("repo.rs");

        fs::write(&service_file, "pub fn process_data() {\n    save_record();\n}\n").unwrap();
        fs::write(&handler_file, "pub fn handle_req() {\n    process_data();\n}\n").unwrap();
        fs::write(&repo_file, "pub fn save_record() {}\n").unwrap();

        let db_dir = temp_dir.join(".agent-context");
        let _ = fs::create_dir_all(&db_dir);
        let db_path = db_dir.join("code_graph.db");
        let db = CodeGraphDb::open(&db_path).expect("open db");

        db.conn.execute(
            "INSERT INTO files (path, content_hash, size, modified_at, indexed_at)
             VALUES ('src/service.rs', 'hash1', 50, 1, 1),
                    ('src/handler.rs', 'hash2', 50, 1, 1),
                    ('src/repo.rs', 'hash3', 50, 1, 1)",
            [],
        ).unwrap();

        db.conn.execute(
            "INSERT INTO symbols (id, file_path, name, kind, start_line, end_line)
             VALUES ('sym_proc', 'src/service.rs', 'process_data', 'function', 1, 3),
                    ('sym_hand', 'src/handler.rs', 'handle_req', 'function', 1, 3),
                    ('sym_save', 'src/repo.rs', 'save_record', 'function', 1, 1)",
            [],
        ).unwrap();

        db.conn.execute(
            "INSERT INTO symbol_edges (source_id, target_id, edge_type, weight, call_line)
             VALUES ('sym_hand', 'sym_proc', 'calls', 1.0, 2),
                    ('sym_proc', 'sym_save', 'calls', 1.0, 2)",
            [],
        ).unwrap();

        (TestProjectGuard { dir: temp_dir }, db)
    }

    #[test]
    fn test_subgraph_bundle_path_spec_syntax() {
        let (guard, _db) = create_mock_project();
        let mut state = ServerState::new();

        let res = handle_subgraph_bundle(
            &json!({}),
            &guard.dir,
            "src/service.rs#process_data",
            &mut state,
        );

        assert!(res.contains("Multi-File Subgraph Bundle: `process_data`"));
        assert!(res.contains("src/service.rs:L1-L3"));
    }

    #[test]
    fn test_subgraph_bundle_authorize_target_only() {
        let (guard, _db) = create_mock_project();
        let mut state = ServerState::new();
        state.workspace_roots = vec![guard.dir.to_string_lossy().to_string()];
        state.workflow_stage = "Build".to_string();
        state.plan_approved = true;

        let res = handle_subgraph_bundle(
            &json!({"authorize": true}),
            &guard.dir,
            "process_data",
            &mut state,
        );

        assert!(res.contains("Direct Edit Authorization Report"));
        let auth_report = res.split("## Direct Edit Authorization Report").nth(1).unwrap();
        assert!(auth_report.contains("src/service.rs"));
        assert!(!auth_report.contains("src/handler.rs"));
        assert!(!auth_report.contains("src/repo.rs"));
    }

    #[test]
    fn test_subgraph_bundle_authorize_all() {
        let (guard, _db) = create_mock_project();
        let mut state = ServerState::new();
        state.workspace_roots = vec![guard.dir.to_string_lossy().to_string()];
        state.workflow_stage = "Build".to_string();
        state.plan_approved = true;

        let res = handle_subgraph_bundle(
            &json!({"authorize": true, "authorize_all": true}),
            &guard.dir,
            "process_data",
            &mut state,
        );

        assert!(res.contains("Direct Edit Authorization Report"));
        assert!(res.contains("src/service.rs"));
        assert!(res.contains("src/handler.rs"));
        assert!(res.contains("src/repo.rs"));
    }

    #[test]
    fn test_subgraph_bundle_deferred_pre_authorization_in_plan_stage() {
        let (guard, _db) = create_mock_project();
        let mut state = ServerState::new();
        state.workspace_roots = vec![guard.dir.to_string_lossy().to_string()];
        state.workflow_stage = "Plan".to_string();
        state.plan_approved = false;

        let res = handle_subgraph_bundle(
            &json!({"authorize": true, "authorize_all": true}),
            &guard.dir,
            "process_data",
            &mut state,
        );

        assert!(res.contains("Deferred Pre-Authorization Queued"));
        assert_eq!(state.deferred_authorized_paths.len(), 3);
        assert!(state.deferred_authorized_paths.contains(&"src/service.rs".to_string()));
        assert!(state.deferred_authorized_paths.contains(&"src/handler.rs".to_string()));
        assert!(state.deferred_authorized_paths.contains(&"src/repo.rs".to_string()));
    }
}
