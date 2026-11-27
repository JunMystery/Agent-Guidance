//! Language-specific test mock integrity and unsandboxed real-state detectors.

use std::collections::{HashMap, HashSet};

use super::test_safety::{has_line_opt_out, has_opt_out, SafetyFinding, SafetySeverity};

pub(super) fn analyze_test_file(
    content: &str,
    rel_path: &str,
    findings: &mut Vec<SafetyFinding>,
) {
    let lower_path = rel_path.to_lowercase();
    if lower_path.ends_with(".py") {
        check_python_mock_namespaces(content, findings);
    } else if [".js", ".ts", ".tsx", ".jsx", ".mjs", ".cjs"]
        .iter()
        .any(|ext| lower_path.ends_with(ext))
    {
        check_js_mock_paths(content, findings);
    }

    check_unverified_side_effect_mocks(content, findings);
    if !has_sandbox_isolation(content) {
        check_unsandboxed_markers(content, findings);
    }
}

fn extract_quoted_literal(s: &str) -> Option<String> {
    for (idx, ch) in s.char_indices() {
        if ch == '"' || ch == '\'' {
            let rest = &s[idx + 1..];
            return rest.find(ch).map(|end| rest[..end].to_string());
        }
    }
    None
}

fn find_python_patch_call(line: &str) -> Option<usize> {
    for needle in ["mock.patch(", "patch("] {
        let mut start = 0;
        while let Some(pos) = line[start..].find(needle) {
            let abs = start + pos;
            let prev = line[..abs].chars().next_back();
            let boundary_ok = match prev {
                None => true,
                Some(c) => !c.is_alphanumeric() && c != '_' && (needle == "mock.patch(" || c != '.'),
            };
            if boundary_ok && !line[abs..].starts_with("patch.object(") {
                return Some(abs);
            }
            start = abs + needle.len();
        }
    }
    None
}

fn check_python_mock_namespaces(content: &str, findings: &mut Vec<SafetyFinding>) {
    let mut qualified: HashMap<String, String> = HashMap::new();
    let mut bare: HashSet<String> = HashSet::new();

    for line in content.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("from ") {
            if let Some((mod_part, _)) = rest.split_once(" import ") {
                let m = mod_part.trim();
                if let Some((_, tail)) = m.rsplit_once('.') {
                    if !m.starts_with('.') {
                        qualified.entry(tail.to_string()).or_insert_with(|| m.to_string());
                    }
                } else if !m.is_empty() && !m.starts_with('.') {
                    bare.insert(m.to_string());
                }
            }
        } else if let Some(rest) = t.strip_prefix("import ") {
            for item in rest.split(',') {
                let m = item.split(" as ").next().unwrap_or("").trim();
                if let Some((_, tail)) = m.rsplit_once('.') {
                    qualified.entry(tail.to_string()).or_insert_with(|| m.to_string());
                } else if !m.is_empty() {
                    bare.insert(m.to_string());
                }
            }
        }
    }

    for (idx, line) in content.lines().enumerate() {
        let t = line.trim();
        if t.starts_with('#') {
            continue;
        }
        let Some(pos) = find_python_patch_call(t) else { continue; };
        let Some(target) = extract_quoted_literal(&t[pos..]) else { continue; };
        let head = target.split('.').next().unwrap_or("");
        if head.is_empty() || bare.contains(head) {
            continue;
        }
        if let Some(full_mod) = qualified.get(head) {
            if !has_line_opt_out(content, t, "mock-namespace") {
                let suffix = target.strip_prefix(head).unwrap_or("");
                findings.push(SafetyFinding {
                    code: "TEST_MOCK_NAMESPACE_MISMATCH",
                    severity: SafetySeverity::Block,
                    line: Some(idx + 1),
                    message: format!(
                        "String patch(\"{}\") targets `{}` while file imports `{}`; Python loads separate module copies and hits live code. Use `patch.object(...)` or `patch(\"{}{}\")`.",
                        target, head, full_mod, full_mod, suffix
                    ),
                });
            }
        }
    }
}

fn strip_js_ext(spec: &str) -> &str {
    [".tsx", ".jsx", ".mjs", ".cjs", ".ts", ".js"]
        .iter()
        .find_map(|ext| spec.strip_suffix(ext))
        .unwrap_or(spec)
}

fn check_js_mock_paths(content: &str, findings: &mut Vec<SafetyFinding>) {
    let mut imported: Vec<String> = Vec::new();
    for line in content.lines() {
        let t = line.trim();
        if t.starts_with("//") {
            continue;
        }
        let slice = t
            .split_once(" from ")
            .map(|(_, r)| r)
            .or_else(|| t.split_once("require(").map(|(_, r)| r));
        if let Some(s) = slice.and_then(extract_quoted_literal) {
            if s.starts_with('.') {
                imported.push(s);
            }
        }
    }

    for (idx, line) in content.lines().enumerate() {
        let t = line.trim();
        if t.starts_with("//") {
            continue;
        }
        let Some(p) = t.find("jest.mock(").or_else(|| t.find("vi.mock(")) else { continue; };
        let Some(mock_spec) = extract_quoted_literal(&t[p..]) else { continue; };
        if !mock_spec.starts_with('.') {
            continue;
        }
        let norm_mock = strip_js_ext(&mock_spec);
        let stem = norm_mock.rsplit('/').next().unwrap_or("");
        if stem.len() < 3 || stem == "index" {
            continue;
        }
        let mismatch = imported.iter().find(|imp| {
            let norm_imp = strip_js_ext(imp);
            norm_imp.rsplit('/').next().unwrap_or("") == stem && norm_imp != norm_mock
        });
        if let Some(actual_imp) = mismatch {
            if !has_line_opt_out(content, t, "mock-namespace") {
                findings.push(SafetyFinding {
                    code: "TEST_MOCK_NAMESPACE_MISMATCH",
                    severity: SafetySeverity::Block,
                    line: Some(idx + 1),
                    message: format!(
                        "Mock path `{}` differs from imported path `{}` (same stem `{}`); verify the mock resolves to the identical module.",
                        mock_spec, actual_imp, stem
                    ),
                });
            }
        }
    }
}

fn check_unverified_side_effect_mocks(content: &str, findings: &mut Vec<SafetyFinding>) {
    if has_opt_out(content, "", "unverified-mock") {
        return;
    }
    let assertions = [
        "assert_called", "assert_not_called", "assert_any_call", "assert_has_calls",
        "call_count", "mock_calls", "toHaveBeenCalled", "toHaveBeenNthCalled", ".called", "verify(",
    ];
    if content.lines().any(|l| {
        let lt = l.trim();
        !lt.starts_with('#') && !lt.starts_with("//") && assertions.iter().any(|kw| lt.contains(kw))
    }) {
        return;
    }
    let side_effects = [
        "save", "write", "delete", "remove", "drop", "commit",
        "store", "storage", "db", "config", "request", "send", "persist", "unlink",
    ];
    for (idx, line) in content.lines().enumerate() {
        let t = line.trim();
        let has_mock = ["patch(", "patch.object(", "jest.mock(", "vi.mock("]
            .iter()
            .any(|m| t.contains(m));
        if has_mock && side_effects.iter().any(|kw| t.to_lowercase().contains(kw)) {
            findings.push(SafetyFinding {
                code: "TEST_UNVERIFIED_SIDE_EFFECT_MOCK",
                severity: SafetySeverity::Advisory,
                line: Some(idx + 1),
                message: "Side-effect mock has no call assertion (`assert_called*` / `toHaveBeenCalled*`). Assert invocation so a bypassed mock fails fast.".to_string(),
            });
            break;
        }
    }
}

fn has_sandbox_isolation(content: &str) -> bool {
    [
        "TemporaryDirectory", "mkdtemp", "tmp_path", "tempfile", "temp_dir()", "TempDir",
        "t.TempDir()", "mkdtempSync", ":memory:", "in_memory", "monkeypatch", "patch.dict",
        "set_var(\"HOME\"", "set_var(\"APPDATA\"", "test-safety-allow: real-state",
    ]
    .iter()
    .any(|tok| content.contains(tok))
}

fn check_unsandboxed_markers(content: &str, findings: &mut Vec<SafetyFinding>) {
    let real_state = [
        "APPDATA", "LOCALAPPDATA", "USERPROFILE", "XDG_CONFIG_HOME", "XDG_DATA_HOME",
        "expanduser(\"~\")", "expanduser('~')", "Path.home()", "os.homedir()",
        "dirs::config_dir", "dirs::data_dir", "dirs::home_dir", "os.UserHomeDir",
        "os.UserConfigDir", "~/.config", "~/.local", "C:\\Users\\", "C:/Users/",
        "DATABASE_URL", "MONGO_URI", "REDIS_URL", ".env.production",
    ];
    for (idx, line) in content.lines().enumerate() {
        let t = line.trim();
        if !t.starts_with('#') && !t.starts_with("//") && real_state.iter().any(|m| t.contains(m)) {
            findings.push(SafetyFinding {
                code: "TEST_UNSANDBOXED_REAL_STATE",
                severity: SafetySeverity::Advisory,
                line: Some(idx + 1),
                message: "Test references user/OS or production state path without a visible temp/in-memory sandbox (`TemporaryDirectory`, `tmp_path`, `temp_dir()`, `:memory:`).".to_string(),
            });
            return;
        }
    }

    if content.contains("patch.object") {
        return;
    }
    let app_inits = ["MainWindow(", "LauncherWindow(", "create_app(", "init_db(", "save_data(", "save_config("];
    for (idx, line) in content.lines().enumerate() {
        let t = line.trim();
        if !t.starts_with('#') && !t.starts_with("//") && app_inits.iter().any(|m| t.contains(m)) {
            findings.push(SafetyFinding {
                code: "TEST_UNSANDBOXED_APP_INIT",
                severity: SafetySeverity::Advisory,
                line: Some(idx + 1),
                message: "Test initializes app/storage directly without a temp sandbox or `patch.object` isolation; verify it cannot mutate real user config files.".to_string(),
            });
            break;
        }
    }
}
