use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::sleep;
use std::time::Duration;

use crate::mcp::state::ServerState;
use crate::mcp::tools::handle_tool_call;

static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

fn create_temp_dir() -> PathBuf {
    let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("ag_cache_test_{}_{}", std::process::id(), id));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).unwrap();
    dir
}

fn call_context(args: serde_json::Value, state: &mut ServerState) -> String {
    let res = handle_tool_call("project_context", args, state).expect("tool call succeeded");
    res["content"][0]["text"].as_str().unwrap_or("").to_string()
}

#[test]
fn test_ast_cache_hit_within_ttl() {
    let temp = create_temp_dir();
    let file = temp.join("src/lib.rs");
    fs::write(&file, "pub fn compute() -> i32 { 42 }").unwrap();

    let mut state = ServerState::new();
    state.workspace_roots = vec![temp.to_string_lossy().to_string()];

    // 1st Read: Cache miss (inserted into cache)
    let res1 = call_context(
        json!({
            "operation": "read",
            "project_path": temp.to_string_lossy(),
            "relative_path": "src/lib.rs"
        }),
        &mut state,
    );
    assert!(res1.contains("pub fn compute"));
    assert!(!res1.contains("AST Response Cache Hit"));
    assert_eq!(state.ast_cache.len(), 1);

    // 2nd Read: Cache hit
    let res2 = call_context(
        json!({
            "operation": "read",
            "project_path": temp.to_string_lossy(),
            "relative_path": "src/lib.rs"
        }),
        &mut state,
    );
    assert!(res2.contains("pub fn compute"));
    assert!(res2.contains("AST Response Cache Hit"));

    let _ = fs::remove_dir_all(&temp);
}

#[test]
fn test_ast_cache_invalidation_on_mtime_change() {
    let temp = create_temp_dir();
    let file = temp.join("src/config.rs");
    fs::write(&file, "pub const V: i32 = 1;").unwrap();

    let mut state = ServerState::new();
    state.workspace_roots = vec![temp.to_string_lossy().to_string()];

    let res1 = call_context(
        json!({
            "operation": "read",
            "project_path": temp.to_string_lossy(),
            "relative_path": "src/config.rs"
        }),
        &mut state,
    );
    assert!(res1.contains("pub const V: i32 = 1;"));

    // Modify file on disk with sleep to guarantee mtime change
    sleep(Duration::from_millis(50));
    fs::write(&file, "pub const V: i32 = 2;").unwrap();

    // 2nd Read: mtime changed -> Cache miss, returns updated content
    let res2 = call_context(
        json!({
            "operation": "read",
            "project_path": temp.to_string_lossy(),
            "relative_path": "src/config.rs"
        }),
        &mut state,
    );
    assert!(res2.contains("pub const V: i32 = 2;"));
    assert!(!res2.contains("AST Response Cache Hit"));

    let _ = fs::remove_dir_all(&temp);
}

#[test]
fn test_ast_cache_invalidation_on_file_modification_signal() {
    let temp = create_temp_dir();
    let file = temp.join("src/handler.rs");
    fs::write(&file, "pub fn handle() {}").unwrap();

    let mut state = ServerState::new();
    state.workspace_roots = vec![temp.to_string_lossy().to_string()];

    let _ = call_context(
        json!({
            "operation": "read",
            "project_path": temp.to_string_lossy(),
            "relative_path": "src/handler.rs"
        }),
        &mut state,
    );
    assert_eq!(state.ast_cache.len(), 1);

    // Invalidate via record_modified_file
    state.record_modified_file("src/handler.rs");
    assert_eq!(state.ast_cache.len(), 0);

    let _ = fs::remove_dir_all(&temp);
}

#[test]
fn test_ast_cache_with_authorize_flag() {
    let temp = create_temp_dir();
    let file = temp.join("src/auth.rs");
    fs::write(&file, "pub fn auth() {}").unwrap();

    let mut state = ServerState::new();
    state.workspace_roots = vec![temp.to_string_lossy().to_string()];
    state.plan_approved = true;
    state.workflow_stage = "Build".to_string();

    // 1st Read without authorize
    let res1 = call_context(
        json!({
            "operation": "read",
            "project_path": temp.to_string_lossy(),
            "relative_path": "src/auth.rs"
        }),
        &mut state,
    );
    assert!(!res1.contains("Direct Edit Authorization Report"));

    // 2nd Read with authorize: true -> Cache hit on body, but runs fresh authorization
    let res2 = call_context(
        json!({
            "operation": "read",
            "project_path": temp.to_string_lossy(),
            "relative_path": "src/auth.rs",
            "authorize": true,
            "justification": "Refactor auth token"
        }),
        &mut state,
    );
    assert!(res2.contains("AST Response Cache Hit"));
    assert!(res2.contains("Direct Edit Authorization Report"));
    assert!(res2.contains("PASSED"));
    assert!(state.edit_authorized);

    let _ = fs::remove_dir_all(&temp);
}

#[test]
fn test_ast_cache_different_slices_isolation() {
    let temp = create_temp_dir();
    let file = temp.join("src/calc.rs");
    fs::write(&file, "pub fn add() -> i32 { 1 }\npub fn sub() -> i32 { 2 }").unwrap();

    let mut state = ServerState::new();
    state.workspace_roots = vec![temp.to_string_lossy().to_string()];

    // Read symbol 'add'
    let res1 = call_context(
        json!({
            "operation": "read",
            "project_path": temp.to_string_lossy(),
            "relative_path": "src/calc.rs",
            "target_symbol": "add"
        }),
        &mut state,
    );
    assert!(res1.contains("Target Symbol Extracted: 'add'"));

    // Read symbol 'sub' -> Different cache key, not hit
    let res2 = call_context(
        json!({
            "operation": "read",
            "project_path": temp.to_string_lossy(),
            "relative_path": "src/calc.rs",
            "target_symbol": "sub"
        }),
        &mut state,
    );
    assert!(res2.contains("Target Symbol Extracted: 'sub'"));
    assert!(!res2.contains("AST Response Cache Hit"));
    assert_eq!(state.ast_cache.len(), 2);

    let _ = fs::remove_dir_all(&temp);
}
