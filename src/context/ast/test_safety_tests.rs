//! Unit tests for static test-safety, mock integrity, and persistence boundary analyzer.

use super::test_safety::{
    analyze_file_safety, analyze_verification_command, SafetySeverity,
};

#[test]
fn test_python_mock_namespace_mismatch_blocks() {
    let src = r#"
from unittest.mock import patch
from launcher.launcher_window_state import hide_window_to_tray

def test_hide_window():
    with patch("launcher_window_state.save_data") as mock_save:
        hide_window_to_tray(None)
        mock_save.assert_called_once()
"#;
    let report = analyze_file_safety(src, "tests/test_tray.py");
    assert!(report.has_blockers());
    assert_eq!(report.findings[0].code, "TEST_MOCK_NAMESPACE_MISMATCH");
    assert_eq!(report.findings[0].severity, SafetySeverity::Block);
}

#[test]
fn test_python_patch_object_and_matching_namespace_pass() {
    let src = r#"
from unittest.mock import patch
import launcher.launcher_window_state as lws

def test_hide_window_safe():
    with patch.object(lws, "save_data") as mock_save:
        mock_save()
        mock_save.assert_called_once()
"#;
    let report = analyze_file_safety(src, "tests/test_tray.py");
    assert!(!report.has_blockers());
    assert!(report.findings.is_empty());

    let qualified_src = r#"
from unittest.mock import patch
from launcher.launcher_window_state import hide_window_to_tray

def test_hide_window_qualified():
    with patch("launcher.launcher_window_state.save_data") as mock_save:
        hide_window_to_tray(None)
        mock_save.assert_called_once()
"#;
    let rep2 = analyze_file_safety(qualified_src, "tests/test_tray.py");
    assert!(!rep2.has_blockers());
}

#[test]
fn test_js_mock_path_mismatch_blocks() {
    let src = r#"
import { saveAccounts } from '../proxy/storage';

jest.mock('./storage');

test('saves safely', () => {
    expect(saveAccounts).toBeDefined();
});
"#;
    let report = analyze_file_safety(src, "tests/storage.test.ts");
    assert!(report.has_blockers());
    assert!(report
        .findings
        .iter()
        .any(|f| f.code == "TEST_MOCK_NAMESPACE_MISMATCH"));
}

#[test]
fn test_unverified_side_effect_mock_advisory() {
    let unverified = r#"
from unittest.mock import patch

def test_save_without_assert():
    with patch("pkg.mod.save_data"):
        assert True
"#;
    let r1 = analyze_file_safety(unverified, "tests/test_store.py");
    assert!(!r1.has_blockers());
    assert!(r1
        .findings
        .iter()
        .any(|f| f.code == "TEST_UNVERIFIED_SIDE_EFFECT_MOCK"));

    let verified = r#"
from unittest.mock import patch

def test_save_with_assert():
    with patch("pkg.mod.save_data") as m:
        m()
        m.assert_called_once()
"#;
    let r2 = analyze_file_safety(verified, "tests/test_store.py");
    assert!(!r2
        .findings
        .iter()
        .any(|f| f.code == "TEST_UNVERIFIED_SIDE_EFFECT_MOCK"));
}

#[test]
fn test_unsandboxed_real_state_advisory_and_sandbox_pass() {
    let unsafe_py = r#"
import os

def test_reads_live_appdata():
    p = os.path.join(os.environ["APPDATA"], "antigravity", "profiles.json")
    assert p
"#;
    let r1 = analyze_file_safety(unsafe_py, "tests/test_paths.py");
    assert!(!r1.has_blockers());
    assert!(r1
        .findings
        .iter()
        .any(|f| f.code == "TEST_UNSANDBOXED_REAL_STATE"));

    let safe_py = r#"
import os
import tempfile

def test_isolated_tmp():
    with tempfile.TemporaryDirectory() as tmp:
        assert os.path.isdir(tmp)
"#;
    let r2 = analyze_file_safety(safe_py, "tests/test_paths.py");
    assert!(r2.findings.is_empty());

    let unsafe_rs = r#"
fn test_live_config() {
    let _p = dirs::config_dir();
}
"#;
    let r3 = analyze_file_safety(unsafe_rs, "src/config_test.rs");
    assert!(r3
        .findings
        .iter()
        .any(|f| f.code == "TEST_UNSANDBOXED_REAL_STATE"));
}

#[test]
fn test_persistence_overwrite_advisory_on_production_storage() {
    let naive_write = r#"
import json

def save_data(path, data):
    with open(path + ".bak", "w") as f:
        json.dump(data, f)
"#;
    let r1 = analyze_file_safety(naive_write, "launcher/launcher_storage.py");
    assert!(r1
        .findings
        .iter()
        .any(|f| f.code == "PERSISTENCE_OVERWRITE_SAFETY"));

    let atomic_write = r#"
import json, os

def save_data(path, data):
    tmp = path + ".tmp"
    with open(tmp, "w") as f:
        json.dump(data, f)
    os.replace(tmp, path)
"#;
    let r2 = analyze_file_safety(atomic_write, "launcher/launcher_storage.py");
    assert!(r2.findings.is_empty());
}

#[test]
fn test_verification_command_blocks_prod_and_allows_safe_tests() {
    let b1 = analyze_verification_command("NODE_ENV=production npm test");
    assert!(b1.has_blockers());
    assert_eq!(b1.findings[0].code, "VERIFY_COMMAND_PROD_TARGET");

    let b2 = analyze_verification_command("psql -c \"DROP DATABASE app\"");
    assert!(b2.has_blockers());

    let ok1 = analyze_verification_command("python -m unittest tests.test_tray -v");
    assert!(!ok1.has_blockers());

    let ok2 = analyze_verification_command("cargo test --bin agent-guidance");
    assert!(!ok2.has_blockers());
}

#[test]
fn test_opt_out_comment_suppresses_block() {
    let src = r#"
from launcher.launcher_window_state import hide_window_to_tray
from unittest.mock import patch

def test_intentional():
    with patch("launcher_window_state.save_data") as m:  # test-safety-allow: mock-namespace
        m()
        m.assert_called_once()
"#;
    let report = analyze_file_safety(src, "tests/test_tray.py");
    assert!(!report.has_blockers());

    // Inline opt-out on one line must NOT suppress a second unannotated mismatch in the same file
    let mixed = r#"
from launcher.launcher_window_state import hide_window_to_tray
from unittest.mock import patch

def test_mixed():
    with patch("launcher_window_state.save_data") as m1:  # test-safety-allow: mock-namespace
        m1()
        m1.assert_called_once()
    with patch("launcher_window_state.other_fn") as m2:
        m2()
        m2.assert_called_once()
"#;
    let mixed_report = analyze_file_safety(mixed, "tests/test_tray.py");
    assert!(mixed_report.has_blockers());
}
