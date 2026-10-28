use super::*;
use std::fs;

#[test]
fn test_detect_project_path_prefers_workspace_root() {
    let temp_root = std::env::temp_dir().join(format!("ag_ws_test_{}", std::process::id()));
    let _ = fs::create_dir_all(&temp_root);

    let mut state = ServerState::new();
    state.workspace_roots = vec![temp_root.to_string_lossy().to_string()];

    // When ".", "./", ".\\" or empty is passed, workspace_roots must take absolute precedence
    for variant in &[".", "./", ".\\", "", "  "] {
        let detected = detect_project_path(variant, &state);
        let canonical_temp = temp_root.canonicalize().unwrap_or(temp_root.clone());
        let canonical_detected = detected.canonicalize().unwrap_or(detected);
        assert_eq!(canonical_detected, canonical_temp, "Variant '{}' failed", variant);
    }

    let _ = fs::remove_dir_all(&temp_root);
}

#[test]
fn test_update_project_path_rejects_foreign_path() {
    let temp_a = std::env::temp_dir().join(format!("ag_ws_up_a_{}", std::process::id()));
    let temp_b = std::env::temp_dir().join(format!("ag_ws_up_b_{}", std::process::id()));
    let _ = fs::create_dir_all(&temp_a);
    let _ = fs::create_dir_all(&temp_b);

    let mut state = ServerState::new();
    state.workspace_roots = vec![temp_a.to_string_lossy().to_string()];

    // Passing foreign path outside workspace_roots must be rejected
    state.update_project_path(&temp_b);
    assert!(state.project_path.is_none(), "Foreign path must not hijack state.project_path");

    // Passing path inside workspace_roots is accepted
    state.update_project_path(&temp_a);
    assert!(state.project_path.is_some(), "Path inside workspace must be accepted");

    let _ = fs::remove_dir_all(&temp_a);
    let _ = fs::remove_dir_all(&temp_b);
}

#[test]
fn test_approve_plan_guards_cross_workspace() {
    let temp_a = std::env::temp_dir().join(format!("ag_ws_ap_a_{}", std::process::id()));
    let temp_b = std::env::temp_dir().join(format!("ag_ws_ap_b_{}", std::process::id()));
    let _ = fs::create_dir_all(temp_a.join("src"));
    let _ = fs::create_dir_all(temp_b.join("src"));
    fs::write(temp_a.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(temp_b.join("src/lib.rs"), "pub fn lib() {}\n").unwrap();

    let mut state = ServerState::new();
    state.workspace_roots = vec![temp_a.to_string_lossy().to_string()];
    state.plan_approved = true;

    // Approving plan with target files in temp_b is BLOCKED without allow_cross_workspace
    let blocked_res = handle_tool_call(
        "workflow_gate",
        json!({
            "action": "approve_plan",
            "user_confirmed": true,
            "project_path": temp_b.to_str().unwrap(),
            "relative_paths": ["src/lib.rs"],
            "justification": "Approved plan with cross-workspace files"
        }),
        &mut state,
    );
    assert!(blocked_res.is_ok());
    let blocked_text = blocked_res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(blocked_text.contains("BLOCKED (CROSS_WORKSPACE_WARNING)"));

    let _ = fs::remove_dir_all(&temp_a);
    let _ = fs::remove_dir_all(&temp_b);
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


#[test]
fn test_native_conversation_id_sync() {
    let mut state = ServerState::new();

    // 1. Initialize with conversationId updates state.session_id
    let init_res = crate::mcp::router::handle_request(
        "initialize",
        Some(json!({
            "clientInfo": { "name": "Antigravity", "version": "1.0" },
            "conversationId": "uuid-antigravity-12345"
        })),
        &mut state,
    );
    assert!(init_res.is_ok());
    assert_eq!(state.session_id, "uuid-antigravity-12345");

    // 2. Tool call with conversation_id updates session_id
    let tool_call_res = crate::mcp::router::handle_request(
        "tools/call",
        Some(json!({
            "name": "workflow_gate",
            "arguments": {
                "action": "check",
                "conversation_id": "uuid-antigravity-updated-999"
            }
        })),
        &mut state,
    );
    assert!(tool_call_res.is_ok());
    assert_eq!(state.session_id, "uuid-antigravity-updated-999");
}

#[test]
fn test_two_stage_skill_search_rejects_unrelated_media() {
    use crate::catalog::store::{SkillItem, SkillSource};
    use crate::catalog::language_detector::ProjectLanguageProfile;

    let candidates = vec![
        SkillItem {
            name: "video-editing".to_string(),
            relative_path: "skills/video-editing/SKILL.md".to_string(),
            source: SkillSource::Embedded,
            content: "# Video Editing\nTimeline cut transitions for video.".to_string(),
        },
        SkillItem {
            name: "github-ops".to_string(),
            relative_path: "skills/github-ops/SKILL.md".to_string(),
            source: SkillSource::Embedded,
            content: "# GitHub Ops\nGit repository push, release and commit operations.".to_string(),
        },
    ];

    let mut primary_langs = std::collections::HashSet::new();
    primary_langs.insert("rust".to_string());
    let mut secondary_tech = std::collections::HashSet::new();
    secondary_tech.insert("cli".to_string());

    let profile = ProjectLanguageProfile {
        primary_languages: primary_langs,
        secondary_tech,
    };

    // Candidate filter must filter out video-editing for rust/cli prompt
    let task = "Pump v1.8.16, update docs. Amend last commit and Push git, rebuild";
    let is_video_compat = crate::ml::llm_selector_filter::is_skill_candidate_compatible(
        &candidates[0],
        &task.to_lowercase(),
        &profile,
    );
    let is_git_compat = crate::ml::llm_selector_filter::is_skill_candidate_compatible(
        &candidates[1],
        &task.to_lowercase(),
        &profile,
    );

    assert!(!is_video_compat, "video-editing must be filtered out by negative domain filter");
    assert!(is_git_compat, "github-ops must be compatible");
}
