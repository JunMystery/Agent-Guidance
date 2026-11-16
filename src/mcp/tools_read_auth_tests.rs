use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::mcp::state::ServerState;
use crate::mcp::tools::handle_tool_call;

static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

fn create_temp_dir() -> PathBuf {
    let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("ag_read_auth_test_{}_{}", std::process::id(), id));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).unwrap();
    dir
}

fn call_context(args: serde_json::Value, state: &mut ServerState) -> String {
    let res = handle_tool_call("project_context", args, state).expect("tool call succeeded");
    res["content"][0]["text"].as_str().unwrap_or("").to_string()
}

#[test]
fn test_single_file_read_and_authorize_in_build_stage() {
    let temp = create_temp_dir();
    let file = temp.join("src/main.rs");
    fs::write(&file, "fn main() { println!(\"hello\"); }").unwrap();

    let mut state = ServerState::new();
    state.workspace_roots = vec![temp.to_string_lossy().to_string()];
    state.plan_approved = true;
    state.workflow_stage = "Build".to_string();

    let res = call_context(
        json!({
            "operation": "read",
            "project_path": temp.to_string_lossy(),
            "relative_path": "src/main.rs",
            "authorize": true,
            "justification": "Refactor entrypoint for modularity"
        }),
        &mut state,
    );

    assert!(res.contains("# Bounded File Content: src/main.rs"));
    assert!(res.contains("## Direct Edit Authorization Report"));
    assert!(res.contains("PASSED"));
    assert!(state.edit_authorized);

    let _ = fs::remove_dir_all(&temp);
}

#[test]
fn test_single_file_read_and_authorize_in_plan_stage() {
    let temp = create_temp_dir();
    let file = temp.join("src/model.rs");
    fs::write(&file, "pub struct Model;").unwrap();

    let mut state = ServerState::new();
    state.workspace_roots = vec![temp.to_string_lossy().to_string()];
    state.plan_approved = false;
    state.workflow_stage = "Plan".to_string();

    let res = call_context(
        json!({
            "operation": "read",
            "project_path": temp.to_string_lossy(),
            "relative_path": "src/model.rs",
            "authorize": true,
            "justification": "Add fields to domain model"
        }),
        &mut state,
    );

    assert!(res.contains("# Bounded File Content: src/model.rs"));
    assert!(res.contains("Deferred Pre-Authorization Queued"));
    assert_eq!(state.drain_deferred_authorized_paths(), vec!["src/model.rs"]);

    let _ = fs::remove_dir_all(&temp);
}

#[test]
fn test_operation_alias_read_and_authorize() {
    let temp = create_temp_dir();
    let file = temp.join("src/handler.rs");
    fs::write(&file, "pub fn handle() {}").unwrap();

    let mut state = ServerState::new();
    state.workspace_roots = vec![temp.to_string_lossy().to_string()];
    state.plan_approved = true;
    state.workflow_stage = "Build".to_string();

    // 1. Test alias 'read_and_authorize'
    let res1 = call_context(
        json!({
            "operation": "read_and_authorize",
            "project_path": temp.to_string_lossy(),
            "relative_path": "src/handler.rs"
        }),
        &mut state,
    );
    assert!(res1.contains("## Direct Edit Authorization Report"));
    assert!(res1.contains("src/handler.rs"));

    // 2. Test alias 'read_auth'
    let res2 = call_context(
        json!({
            "operation": "read_auth",
            "project_path": temp.to_string_lossy(),
            "relative_path": "src/handler.rs"
        }),
        &mut state,
    );
    assert!(res2.contains("## Direct Edit Authorization Report"));

    let _ = fs::remove_dir_all(&temp);
}

#[test]
fn test_single_file_read_auth_modularity_guard() {
    let temp = create_temp_dir();

    let mut state = ServerState::new();
    state.workspace_roots = vec![temp.to_string_lossy().to_string()];
    state.plan_approved = false;
    state.workflow_stage = "Plan".to_string();

    let res = call_context(
        json!({
            "operation": "read_and_authorize",
            "project_path": temp.to_string_lossy(),
            "relative_path": "src/shared_services.rs",
            "justification": "Add plural service container"
        }),
        &mut state,
    );

    assert!(res.contains("Pre-Authorization Validation Blocked"));
    assert!(res.contains("COMPOUND_FILE_NAME"));
    assert!(state.drain_deferred_authorized_paths().is_empty());

    let _ = fs::remove_dir_all(&temp);
}

#[test]
fn test_single_file_read_pending_new_file() {
    let temp = create_temp_dir();

    let mut state = ServerState::new();
    state.workspace_roots = vec![temp.to_string_lossy().to_string()];
    state.plan_approved = false;
    state.workflow_stage = "Plan".to_string();

    let res = call_context(
        json!({
            "operation": "read",
            "project_path": temp.to_string_lossy(),
            "relative_path": "src/domain/user.rs",
            "authorize": true
        }),
        &mut state,
    );

    assert!(res.contains("# Pending New File: src/domain/user.rs"));
    assert!(res.contains("// [NEW FILE - Pending Creation]"));
    assert!(res.contains("Deferred Pre-Authorization Queued"));
    assert_eq!(state.drain_deferred_authorized_paths(), vec!["src/domain/user.rs"]);

    let _ = fs::remove_dir_all(&temp);
}
