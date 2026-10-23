use super::*;

#[test]
fn test_push_recent_task_ring_buffer() {
    let mut state = ServerState::new();
    assert!(state.recent_tasks.is_empty());

    state.push_recent_task("Task 1: Setup auth");
    assert_eq!(state.recent_tasks.len(), 1);
    assert_eq!(state.recent_tasks[0], "Task 1: Setup auth");

    // Consecutive duplicate should be ignored
    state.push_recent_task("Task 1: Setup auth");
    assert_eq!(state.recent_tasks.len(), 1);

    // Empty or generic should be ignored
    state.push_recent_task("");
    state.push_recent_task("general task");
    assert_eq!(state.recent_tasks.len(), 1);

    // Push more tasks
    state.push_recent_task("Task 2: Implement JWT");
    state.push_recent_task("Task 3: Add test suite");
    state.push_recent_task("Task 4: Fix token expiry");
    state.push_recent_task("Task 5: Refactor middleware");
    assert_eq!(state.recent_tasks.len(), 5);

    // 6th task should evict oldest (Task 1)
    state.push_recent_task("Task 6: Audit security");
    assert_eq!(state.recent_tasks.len(), 5);
    assert_eq!(state.recent_tasks[0], "Task 2: Implement JWT");
    assert_eq!(state.recent_tasks[4], "Task 6: Audit security");
}

#[test]
fn test_guidance_search_displays_context_source() {
    let mut state = ServerState::new();
    state.active_task = Some("Refactor database indexing".to_string());
    state.push_recent_task("Optimize query latency");
    state.record_modified_file("src/db/index.rs");

    let res = handle_tool_call(
        "guidance",
        json!({
            "operation": "search",
            "query": "performance metrics"
        }),
        &mut state,
    );
    assert!(res.is_ok());
    let text = res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(text.contains("Context Source:"));
    assert!(text.contains("Active: Refactor database indexing") || text.contains("Optimize query latency"));
}
