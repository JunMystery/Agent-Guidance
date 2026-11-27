use super::*;

#[test]
fn test_context_cluster_read_multiple_files() {
    let mut state = ServerState::new();

    let res = handle_tool_call(
        "project_context",
        json!({
            "operation": "read",
            "project_path": ".",
            "relative_paths": ["Cargo.toml", "src/main.rs"]
        }),
        &mut state,
    );

    assert!(res.is_ok(), "Clustered read should succeed: {:?}", res);
    let text = res.unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();

    assert!(text.contains("# Multi-File Clustered Context (2 file(s)"));
    assert!(text.contains("File: `Cargo.toml`"));
    assert!(text.contains("File: `src/main.rs`"));
    assert!(text.contains("[package]"));
    assert!(text.contains("One-Turn Edit Authorization"));
    assert!(text.contains("workflow_gate(action=\"authorize_edit\""));
    assert!(text.contains("relative_paths=[\"Cargo.toml\",\"src/main.rs\"]") || text.contains("Cargo.toml"));
}

#[test]
fn test_context_cluster_read_alias() {
    let mut state = ServerState::new();

    let res = handle_tool_call(
        "project_context",
        json!({
            "operation": "cluster_read",
            "project_path": ".",
            "relative_paths": ["Cargo.toml"]
        }),
        &mut state,
    );

    assert!(res.is_ok(), "Operation cluster_read alias should work: {:?}", res);
    let text = res.unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(text.contains("# Multi-File Clustered Context (1 file(s)"));
}

#[test]
fn test_context_cluster_read_traversal_protection() {
    let mut state = ServerState::new();

    let res = handle_tool_call(
        "project_context",
        json!({
            "operation": "read",
            "project_path": ".",
            "relative_paths": ["Cargo.toml", "../../../etc/shadow"]
        }),
        &mut state,
    );

    assert!(res.is_ok());
    let text = res.unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(text.contains("PATH_TRAVERSAL_PROHIBITED"));
    assert!(text.contains("File: `Cargo.toml`"));
}

#[test]
fn test_context_cluster_read_large_file_skeleton() {
    let tmp_dir = std::env::temp_dir().join(format!("cluster_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp_dir);
    let large_file = tmp_dir.join("large.rs");
    let content = (1..=320)
        .map(|i| format!("fn test_fn_{}() {{ println!(\"{}\"); }}", i, i))
        .collect::<Vec<_>>()
        .join("\n");
    let _ = std::fs::write(&large_file, &content);

    let mut state = ServerState::new();

    let res = handle_tool_call(
        "project_context",
        json!({
            "operation": "read",
            "project_path": tmp_dir.to_str().unwrap(),
            "relative_paths": ["large.rs"]
        }),
        &mut state,
    );

    assert!(res.is_ok());
    let text = res.unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(text.contains("AST Structural Skeleton") || text.contains("300 LOC Exceeded"));

    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn test_context_cluster_read_empty_file_line_range_does_not_panic() {
    let tmp_dir = std::env::temp_dir().join(format!("cluster_empty_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp_dir);
    std::fs::write(tmp_dir.join("empty.rs"), "").unwrap();

    let mut state = ServerState::new();
    let res = handle_tool_call(
        "project_context",
        json!({
            "operation": "read",
            "project_path": tmp_dir.to_str().unwrap(),
            "relative_paths": ["empty.rs:1-10"]
        }),
        &mut state,
    )
    .unwrap();
    let text = res["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Empty file, 0 lines"));

    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn test_cluster_read_warns_over_8_caps_exempt_and_supports_linked() {
    let base = std::env::temp_dir().join(format!("cluster_h13_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let repo = base.join("repo");
    let linked = base.join("linked_lib");
    std::fs::create_dir_all(repo.join(".agent-context")).unwrap();
    std::fs::create_dir_all(linked.join("src")).unwrap();

    for i in 1..=9 {
        std::fs::write(repo.join(format!("f{}.rs", i)), format!("pub fn f{}() {{}}\n", i)).unwrap();
    }
    let big_lock: String = (1..=450).map(|i| format!("pkg_{} = \"1.0.0\"", i)).collect::<Vec<_>>().join("\n");
    std::fs::write(repo.join("Cargo.lock"), big_lock).unwrap();
    std::fs::write(linked.join("src/util.rs"), "pub fn linked_util() -> i32 { 42 }\n").unwrap();
    std::fs::write(
        repo.join(".agent-context/linked_projects.json"),
        serde_json::to_string(&vec![serde_json::json!({
            "name": "linked_lib",
            "path": linked.to_string_lossy()
        })])
        .unwrap(),
    )
    .unwrap();

    let mut state = ServerState::new();
    let res_over = handle_tool_call(
        "project_context",
        json!({
            "operation": "read",
            "project_path": repo.to_str().unwrap(),
            "relative_paths": ["f1.rs", "f2.rs", "f3.rs", "f4.rs", "f5.rs", "f6.rs", "f7.rs", "f8.rs", "f9.rs"]
        }),
        &mut state,
    )
    .unwrap();
    let over_text = res_over["content"][0]["text"].as_str().unwrap();
    assert!(over_text.contains("Cluster Limit Exceeded"), "Got: {}", over_text);
    assert!(over_text.contains("f9.rs"), "Omitted file should be listed: {}", over_text);

    let res_exempt_and_linked = handle_tool_call(
        "project_context",
        json!({
            "operation": "read",
            "project_path": repo.to_str().unwrap(),
            "relative_paths": ["Cargo.lock", "linked:linked_lib/src/util.rs"]
        }),
        &mut state,
    )
    .unwrap();
    let mix_text = res_exempt_and_linked["content"][0]["text"].as_str().unwrap();
    assert!(mix_text.contains("Exempt file capped at 300 of 450 lines"), "Got: {}", mix_text);
    assert!(!mix_text.contains("pkg_440"), "Lockfile should be capped at 300 lines");
    assert!(mix_text.contains("pub fn linked_util()"), "Got: {}", mix_text);

    let _ = std::fs::remove_dir_all(&base);
}
