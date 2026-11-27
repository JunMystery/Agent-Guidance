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

    assert!(res.contains("Deferred Pre-Authorization Queued"));
    assert!(res.contains("DOMAIN_COHESION_ADVISORY"));
    assert_eq!(state.drain_deferred_authorized_paths(), vec!["src/shared_services.rs"]);

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

#[test]
fn test_read_and_authorize_enforces_fast_path_and_cross_workspace_guards() {
    let temp = create_temp_dir();
    fs::write(temp.join("src/a.rs"), "pub fn a() {}").unwrap();
    fs::write(temp.join("src/b.rs"), "pub fn b() {}").unwrap();

    let mut state = ServerState::new();
    state.workspace_roots = vec![temp.to_string_lossy().to_string()];
    state.plan_approved = true;
    state.workflow_stage = "Build".to_string();
    state.is_fast_path = true;

    let res = call_context(
        json!({
            "operation": "read",
            "project_path": temp.to_string_lossy(),
            "relative_paths": ["src/a.rs", "src/b.rs"],
            "authorize": true
        }),
        &mut state,
    );
    assert!(res.contains("FAST_PATH_BOUNDARY_EXCEEDED"));
    assert!(!state.is_fast_path);
    assert!(!state.plan_approved);

    let _ = fs::remove_dir_all(&temp);
}

#[test]
fn test_read_authorize_blocked_outside_build_and_cross_workspace_single_auth_works() {
    let base = std::env::temp_dir().join(format!("ag_cross_single_{}", std::process::id()));
    let repo_a = base.join("repo_a");
    let repo_b = base.join("repo_b");
    let _ = fs::create_dir_all(repo_a.join("src"));
    let _ = fs::create_dir_all(repo_b.join("src"));
    let main_a = repo_a.join("src/main.rs");
    let ext_b = repo_b.join("src/ext.rs");
    fs::write(&main_a, "fn main() {}\n").unwrap();
    fs::write(&ext_b, "pub fn ext() {}\n").unwrap();

    let mut state = ServerState::new();
    state.workspace_roots = vec![repo_a.to_string_lossy().to_string()];
    state.plan_approved = true;
    state.workflow_stage = "Proposal".to_string();

    let prop_res = call_context(
        json!({
            "operation": "read",
            "project_path": repo_a.to_string_lossy(),
            "relative_path": "src/main.rs",
            "authorize": true
        }),
        &mut state,
    );
    assert!(prop_res.contains("WORKFLOW_STAGE_BLOCKED"));
    assert!(!state.edit_authorized);

    state.workflow_stage = "Build".to_string();
    let cross_res = handle_tool_call(
        "workflow_gate",
        json!({
            "action": "authorize_edit",
            "project_path": repo_a.to_string_lossy(),
            "relative_path": ext_b.to_string_lossy(),
            "architecture_pattern": "Flat_Library",
            "allow_cross_workspace": true,
            "justification": "Cross-workspace edit explicitly permitted"
        }),
        &mut state,
    )
    .unwrap();
    let cross_text = cross_res["content"][0]["text"].as_str().unwrap();
    assert!(cross_text.contains("Status: PASSED"), "Got: {}", cross_text);
    assert!(state.edit_authorized);

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn test_linked_cross_project_read_authorize_requires_cross_workspace_and_targets_linked_root() {
    let base = std::env::temp_dir().join(format!("ag_linked_c4_{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let primary = base.join("primary");
    let linked_pkg = base.join("linked_pkg");
    fs::create_dir_all(primary.join("src")).unwrap();
    fs::create_dir_all(primary.join(".agent-context")).unwrap();
    fs::create_dir_all(linked_pkg.join("src")).unwrap();

    fs::write(primary.join("src/shared.rs"), "pub fn primary_only() {}\n").unwrap();
    fs::write(linked_pkg.join("src/shared.rs"), "pub fn linked_only() {}\n").unwrap();
    fs::write(
        primary.join(".agent-context/linked_projects.json"),
        serde_json::to_string(&vec![serde_json::json!({
            "name": "linked_pkg",
            "path": linked_pkg.to_string_lossy()
        })])
        .unwrap(),
    )
    .unwrap();

    let mut state = ServerState::new();
    state.workspace_roots = vec![primary.to_string_lossy().to_string()];
    state.plan_approved = true;
    state.workflow_stage = "Build".to_string();

    // 1. Without allow_cross_workspace=true, linked authorize=true must be BLOCKED and never snapshot primary
    let blocked = call_context(
        json!({
            "operation": "read",
            "project_path": primary.to_string_lossy(),
            "relative_path": "linked:linked_pkg/src/shared.rs",
            "authorize": true
        }),
        &mut state,
    );
    assert!(blocked.contains("CROSS_WORKSPACE_WARNING"), "Expected cross-workspace block, got: {}", blocked);
    assert!(!state.edit_authorized);
    assert!(!primary.join(".agent-context/snapshots").exists());

    // 2. With allow_cross_workspace=true, authorization and snapshot target linked_pkg root, never primary
    let allowed = call_context(
        json!({
            "operation": "read",
            "project_path": primary.to_string_lossy(),
            "relative_path": "linked:linked_pkg/src/shared.rs",
            "authorize": true,
            "allow_cross_workspace": true,
            "architecture_pattern": "Flat_Library",
            "justification": "Authorized cross-project linked edit"
        }),
        &mut state,
    );
    assert!(allowed.contains("pub fn linked_only()"), "Got: {}", allowed);
    assert!(allowed.contains("PASSED"), "Got: {}", allowed);
    assert!(!primary.join(".agent-context/snapshots").exists(), "Primary must never be snapshotted for linked read");
    assert!(linked_pkg.join(".agent-context/snapshots").exists(), "Linked project must receive the snapshot");

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn test_linked_cross_project_read_authorize_in_plan_stage_preserves_target_path() {
    let base = std::env::temp_dir().join(format!("ag_linked_plan_{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let primary = base.join("primary");
    let linked_pkg = base.join("linked_pkg");
    fs::create_dir_all(primary.join("src")).unwrap();
    fs::create_dir_all(primary.join(".agent-context")).unwrap();
    fs::create_dir_all(linked_pkg.join("src")).unwrap();

    fs::write(primary.join("src/shared.rs"), "pub fn primary_only() {}\n").unwrap();
    fs::write(linked_pkg.join("src/shared.rs"), "pub fn linked_only() {}\n").unwrap();
    fs::write(
        primary.join(".agent-context/linked_projects.json"),
        serde_json::to_string(&vec![serde_json::json!({
            "name": "linked_pkg",
            "path": linked_pkg.to_string_lossy()
        })])
        .unwrap(),
    )
    .unwrap();

    let mut state = ServerState::new();
    state.workspace_roots = vec![primary.to_string_lossy().to_string()];
    state.plan_approved = false;
    state.workflow_stage = "Plan".to_string();

    let res = call_context(
        json!({
            "operation": "read",
            "project_path": primary.to_string_lossy(),
            "relative_path": "linked:linked_pkg/src/shared.rs",
            "authorize": true,
            "allow_cross_workspace": true,
            "justification": "Defer cross-project linked edit in Plan stage"
        }),
        &mut state,
    );
    assert!(res.contains("Deferred Pre-Authorization Queued"), "Got: {}", res);
    assert_eq!(state.deferred_authorized_paths.len(), 1);
    let queued = &state.deferred_authorized_paths[0];
    assert_ne!(
        queued, "src/shared.rs",
        "Must not strip linked root to bare relative subpath"
    );
    assert!(
        queued.ends_with("linked_pkg/src/shared.rs"),
        "Expected full linked target path in deferred queue, got: {}",
        queued
    );

    let approve_res = handle_tool_call(
        "workflow_gate",
        json!({
            "action": "approve_plan",
            "project_path": primary.to_string_lossy(),
            "user_confirmed": true,
            "allow_cross_workspace": true,
            "architecture_pattern": "Flat_Library"
        }),
        &mut state,
    )
    .unwrap();
    let approve_text = approve_res["content"][0]["text"].as_str().unwrap();
    assert!(approve_text.contains("Status: PASSED"), "Got: {}", approve_text);
    assert!(approve_text.contains("linked_pkg/src/shared.rs"), "Got: {}", approve_text);
    assert!(state.edit_authorized);
    assert!(state.deferred_authorized_paths.is_empty());

    let _ = fs::remove_dir_all(&base);
}
