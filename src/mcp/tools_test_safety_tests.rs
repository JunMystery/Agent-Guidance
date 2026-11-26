//! MCP integration tests for Test-Safety & Production-Data Boundary Guard in verify and advance gates.

use crate::mcp::state::ServerState;
use crate::mcp::tools::{handle_tool_call, json};

#[test]
fn test_guidance_verify_blocks_mock_namespace_mismatch_and_recovers() {
    let temp_dir = std::env::temp_dir().join(format!("ag_safety_verify_{}", std::process::id()));
    let tests_dir = temp_dir.join("tests");
    let _ = std::fs::create_dir_all(&tests_dir);
    let test_file = tests_dir.join("test_tray.py");
    std::fs::write(
        &test_file,
        "from unittest.mock import patch\nfrom launcher.launcher_window_state import hide\n\ndef test_hide():\n    with patch(\"launcher_window_state.save_data\") as m:\n        m()\n        m.assert_called_once()\n",
    )
    .unwrap();

    let mut state = ServerState::new();
    state.plan_approved = true;
    state.set_stage("Build").unwrap();
    state.update_project_path(&temp_dir);
    state.record_modified_file("tests/test_tray.py");

    let blocked = handle_tool_call(
        "guidance",
        json!({
            "operation": "verify",
            "project_path": temp_dir.to_str().unwrap(),
            "verification_command": "python -m unittest tests.test_tray",
            "expected_output_keyword": "OK"
        }),
        &mut state,
    )
    .unwrap();
    let blocked_text = blocked["content"][0]["text"].as_str().unwrap();
    assert!(blocked_text.contains("TEST_SAFETY_VIOLATION"));
    assert!(blocked_text.contains("TEST_MOCK_NAMESPACE_MISMATCH"));
    assert!(!state.verification_passed);

    // Fix by switching to patch.object
    std::fs::write(
        &test_file,
        "from unittest.mock import patch\nimport launcher.launcher_window_state as lws\n\ndef test_hide():\n    with patch.object(lws, \"save_data\") as m:\n        m()\n        m.assert_called_once()\n",
    )
    .unwrap();

    let passed = handle_tool_call(
        "guidance",
        json!({
            "operation": "verify",
            "project_path": temp_dir.to_str().unwrap(),
            "verification_command": "python -m unittest tests.test_tray",
            "expected_output_keyword": "OK"
        }),
        &mut state,
    )
    .unwrap();
    let passed_text = passed["content"][0]["text"].as_str().unwrap();
    assert!(passed_text.contains("Empirical Verification Contract Registered"));
    assert!(passed_text.contains("Live-Data Immutability Check"));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_guidance_verify_blocks_production_command_and_advises_unsandboxed_test() {
    let temp_dir = std::env::temp_dir().join(format!("ag_safety_cmd_{}", std::process::id()));
    let tests_dir = temp_dir.join("tests");
    let _ = std::fs::create_dir_all(&tests_dir);
    std::fs::write(
        tests_dir.join("test_env.py"),
        "import os\ndef test_cfg():\n    _ = os.environ.get('APPDATA')\n",
    )
    .unwrap();

    let mut state = ServerState::new();
    state.plan_approved = true;
    state.set_stage("Build").unwrap();
    state.update_project_path(&temp_dir);
    state.record_modified_file("tests/test_env.py");

    let cmd_blocked = handle_tool_call(
        "guidance",
        json!({
            "operation": "verify",
            "project_path": temp_dir.to_str().unwrap(),
            "verification_command": "NODE_ENV=production npm test",
            "expected_output_keyword": "pass"
        }),
        &mut state,
    )
    .unwrap();
    let cmd_text = cmd_blocked["content"][0]["text"].as_str().unwrap();
    assert!(cmd_text.contains("TEST_SAFETY_VIOLATION"));
    assert!(cmd_text.contains("VERIFY_COMMAND_PROD_TARGET"));

    let advisory_ok = handle_tool_call(
        "guidance",
        json!({
            "operation": "verify",
            "project_path": temp_dir.to_str().unwrap(),
            "verification_command": "pytest tests/test_env.py",
            "expected_output_keyword": "passed"
        }),
        &mut state,
    )
    .unwrap();
    let adv_text = advisory_ok["content"][0]["text"].as_str().unwrap();
    assert!(adv_text.contains("Empirical Verification Contract Registered"));
    assert!(adv_text.contains("TEST_UNSANDBOXED_REAL_STATE"));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_workflow_gate_advance_blocks_test_safety_violation() {
    let temp_dir = std::env::temp_dir().join(format!("ag_safety_adv_{}", std::process::id()));
    let tests_dir = temp_dir.join("tests");
    let _ = std::fs::create_dir_all(&tests_dir);
    std::fs::write(
        tests_dir.join("test_store.py"),
        "from unittest.mock import patch\nfrom pkg.storage import save_data\n\ndef test_save():\n    with patch(\"storage.save_data\") as m:\n        m()\n        m.assert_called_once()\n",
    )
    .unwrap();

    let mut state = ServerState::new();
    state.plan_approved = true;
    state.set_stage("Build").unwrap();
    state.update_project_path(&temp_dir);
    state.record_modified_file("tests/test_store.py");

    let res = handle_tool_call(
        "workflow_gate",
        json!({
            "action": "advance",
            "project_path": temp_dir.to_str().unwrap(),
            "target_stage": "Review"
        }),
        &mut state,
    )
    .unwrap();
    let text = res["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("TEST_SAFETY_VIOLATION"));
    assert_eq!(state.workflow_stage, "Build");

    let _ = std::fs::remove_dir_all(&temp_dir);
}
