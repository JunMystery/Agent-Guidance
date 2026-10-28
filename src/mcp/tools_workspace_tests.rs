use super::*;
use std::fs;

#[test]
fn test_detect_project_path_prefers_workspace_root() {
    let temp_root = std::env::temp_dir().join(format!("ag_ws_test_{}", std::process::id()));
    let _ = fs::create_dir_all(&temp_root);

    let mut state = ServerState::new();
    state.workspace_roots = vec![temp_root.to_string_lossy().to_string()];

    // When "." or empty is passed, workspace_roots must take absolute precedence
    let detected = detect_project_path(".", &state);
    let canonical_temp = temp_root.canonicalize().unwrap_or(temp_root.clone());
    let canonical_detected = detected.canonicalize().unwrap_or(detected);
    assert_eq!(canonical_detected, canonical_temp);

    let _ = fs::remove_dir_all(&temp_root);
}

#[test]
fn test_cross_workspace_warning_gate() {
    let temp_a = std::env::temp_dir().join(format!("ag_ws_a_{}", std::process::id()));
    let temp_b = std::env::temp_dir().join(format!("ag_ws_b_{}", std::process::id()));
    let _ = fs::create_dir_all(temp_a.join("src"));
    let _ = fs::create_dir_all(temp_b.join("src"));
    fs::write(temp_a.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(temp_b.join("src/lib.rs"), "pub fn lib() {}\n").unwrap();

    let mut state = ServerState::new();
    state.workspace_roots = vec![temp_a.to_string_lossy().to_string()];
    state.plan_approved = true;
    state.workflow_stage = "Build".to_string();

    // 1. Attempting to authorize file in temp_b without allow_cross_workspace is BLOCKED with warning
    let blocked_res = handle_tool_call(
        "workflow_gate",
        json!({
            "action": "authorize_edit",
            "project_path": temp_b.to_str().unwrap(),
            "relative_path": "src/lib.rs",
            "architecture_pattern": "Flat_Library",
            "justification": "Cross workspace test"
        }),
        &mut state,
    );
    assert!(blocked_res.is_ok());
    let blocked_text = blocked_res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(blocked_text.contains("BLOCKED (CROSS_WORKSPACE_WARNING)"));
    assert!(blocked_text.contains("allow_cross_workspace=true"));

    // 2. Supplying allow_cross_workspace: true allows authorization to proceed
    let allowed_res = handle_tool_call(
        "workflow_gate",
        json!({
            "action": "authorize_edit",
            "project_path": temp_b.to_str().unwrap(),
            "relative_path": "src/lib.rs",
            "architecture_pattern": "Flat_Library",
            "justification": "Cross workspace test approved",
            "allow_cross_workspace": true
        }),
        &mut state,
    );
    assert!(allowed_res.is_ok());
    let allowed_text = allowed_res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(allowed_text.contains("Status: PASSED"));

    let _ = fs::remove_dir_all(&temp_a);
    let _ = fs::remove_dir_all(&temp_b);
}
