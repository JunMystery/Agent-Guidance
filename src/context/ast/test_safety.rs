//! Static test-safety, mock integrity, and persistence-boundary analyzer (Classes A-D).

use std::path::Path;

use super::metrics::is_test_file_path;
use super::test_safety_checks::analyze_test_file;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafetySeverity {
    Block,
    Advisory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafetyFinding {
    pub code: &'static str,
    pub severity: SafetySeverity,
    pub line: Option<usize>,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct SafetyReport {
    pub findings: Vec<SafetyFinding>,
}

impl SafetyReport {
    pub fn has_blockers(&self) -> bool {
        self.findings.iter().any(|f| f.severity == SafetySeverity::Block)
    }

    pub fn format_blockers(&self, rel_path: &str) -> Option<String> {
        self.format_by_severity(rel_path, Some(SafetySeverity::Block))
    }

    pub fn format_advisories(&self, rel_path: &str) -> Option<String> {
        self.format_by_severity(rel_path, Some(SafetySeverity::Advisory))
    }

    pub fn format_all(&self, rel_path: &str) -> Option<String> {
        self.format_by_severity(rel_path, None)
    }

    fn format_by_severity(&self, rel_path: &str, filter: Option<SafetySeverity>) -> Option<String> {
        let matched: Vec<&SafetyFinding> = self
            .findings
            .iter()
            .filter(|f| filter.map_or(true, |sev| f.severity == sev))
            .collect();
        if matched.is_empty() {
            return None;
        }
        let lines: Vec<String> = matched
            .into_iter()
            .map(|f| {
                let loc = f.line.map(|l| format!(":L{}", l)).unwrap_or_default();
                let tag = if f.severity == SafetySeverity::Block { "BLOCKER" } else { "ADVISORY" };
                format!("- [{}] **{}** (`{}{}`): {}", tag, f.code, rel_path, loc, f.message)
            })
            .collect();
        Some(lines.join("\n"))
    }
}

pub fn analyze_safety_path(full_path: &Path, rel_path: &str) -> Option<SafetyReport> {
    if !full_path.exists() || !full_path.is_file() {
        return None;
    }
    if crate::mcp::tools::gate_exemptions::is_exempt_from_loc_limit(rel_path) {
        return None;
    }
    let content = std::fs::read_to_string(full_path).ok()?;
    Some(analyze_file_safety(&content, rel_path))
}

pub fn analyze_file_safety(content: &str, rel_path: &str) -> SafetyReport {
    let mut findings = Vec::new();
    if is_test_file_path(rel_path) {
        analyze_test_file(content, rel_path, &mut findings);
    } else {
        analyze_persistence_file(content, rel_path, &mut findings);
    }
    SafetyReport { findings }
}

fn strip_shell_comments(cmd: &str) -> String {
    let mut out = String::with_capacity(cmd.len());
    for (idx, line) in cmd.lines().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        let mut in_single = false;
        let mut in_double = false;
        let mut escaped = false;
        let mut prev_ws = true;
        for (byte_idx, ch) in line.char_indices() {
            if escaped {
                out.push(ch);
                escaped = false;
                prev_ws = ch.is_whitespace();
                continue;
            }
            if ch == '\\' && !in_single {
                out.push(ch);
                escaped = true;
                prev_ws = false;
                continue;
            }
            if ch == '\'' && !in_double {
                in_single = !in_single;
                out.push(ch);
                prev_ws = false;
                continue;
            }
            if ch == '"' && !in_single {
                in_double = !in_double;
                out.push(ch);
                prev_ws = false;
                continue;
            }
            if ch == '#' && !in_single && !in_double && (byte_idx == 0 || prev_ws) {
                break;
            }
            out.push(ch);
            prev_ws = ch.is_whitespace();
        }
    }
    out
}

fn has_valid_allow_token(uncommented_lower: &str) -> bool {
    let needle = "test-safety-allow:";
    let mut start = 0;
    while let Some(pos) = uncommented_lower[start..].find(needle) {
        let rest = uncommented_lower[start + pos + needle.len()..].trim_start();
        let tag: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        if !tag.is_empty() {
            return true;
        }
        start += pos + needle.len();
    }
    false
}

fn contains_rm_rf_root(lower: &str) -> bool {
    for pattern in ["rm -rf /", "rm -fr /"] {
        let mut start = 0;
        while let Some(pos) = lower[start..].find(pattern) {
            let after_idx = start + pos + pattern.len();
            let next_ch = lower[after_idx..].chars().next();
            if matches!(
                next_ch,
                None | Some(' ') | Some('\t') | Some('\n') | Some('\r') | Some('*') | Some(';') | Some('&') | Some('|') | Some('"') | Some('\'')
            ) {
                return true;
            }
            start = after_idx;
        }
    }
    false
}

pub fn analyze_verification_command(cmd: &str) -> SafetyReport {
    let uncommented = strip_shell_comments(cmd);
    let lower = uncommented.to_lowercase();

    if has_valid_allow_token(&lower) {
        return SafetyReport::default();
    }

    let prod_markers = [
        "node_env=production",
        "app_env=production",
        "rails_env=production",
        "flask_env=production",
        "--env=prod",
        "--env prod",
        "--profile prod",
        "drop database",
        "truncate table",
    ];
    for marker in prod_markers {
        if lower.contains(marker) {
            return SafetyReport {
                findings: vec![SafetyFinding {
                    code: "VERIFY_COMMAND_PROD_TARGET",
                    severity: SafetySeverity::Block,
                    line: None,
                    message: format!(
                        "Verification command targets production or destructive state (`{}`). Run against an isolated test/in-memory target instead.",
                        marker
                    ),
                }],
            };
        }
    }
    if contains_rm_rf_root(&lower) {
        return SafetyReport {
            findings: vec![SafetyFinding {
                code: "VERIFY_COMMAND_PROD_TARGET",
                severity: SafetySeverity::Block,
                line: None,
                message: "Verification command contains destructive root removal (`rm -rf /`). Run against an isolated test/in-memory target instead.".to_string(),
            }],
        };
    }

    let _has_safe_override = lower.contains(":memory:")
        || lower.contains("--dry-run")
        || lower.contains("test_db");
    SafetyReport::default()
}

pub(super) fn has_opt_out(content: &str, line: &str, tag: &str) -> bool {
    let exact = format!("test-safety-allow: {}", tag);
    let compact = format!("test-safety-allow:{}", tag);
    let check = |s: &str| s.contains(&exact) || s.contains(&compact) || s.contains("test-safety-allow: all");
    check(line) || check(content)
}

pub(super) fn has_line_opt_out(content: &str, line: &str, tag: &str) -> bool {
    let exact = format!("test-safety-allow: {}", tag);
    let compact = format!("test-safety-allow:{}", tag);
    let file_exact = format!("test-safety-allow-file: {}", tag);
    line.contains(&exact)
        || line.contains(&compact)
        || content.contains(&file_exact)
        || content.contains("test-safety-allow: all")
}

fn analyze_persistence_file(content: &str, rel_path: &str, findings: &mut Vec<SafetyFinding>) {
    if has_opt_out(content, "", "persistence") {
        return;
    }
    let lower_path = rel_path.to_lowercase();
    let is_persistence_path = ["storage", "store", "config", "state", "persist", "backup", "save"]
        .iter()
        .any(|kw| lower_path.contains(kw))
        || content.contains(".bak")
        || content.contains("save_data")
        || content.contains("save_config");
    if !is_persistence_path {
        return;
    }

    let write_line = content.lines().enumerate().find_map(|(idx, line)| {
        let t = line.trim();
        let has_write = ["json.dump(", "write_text(", "writeFileSync(", "std::fs::write(", "os.WriteFile("]
            .iter()
            .any(|w| t.contains(w));
        if has_write { Some(idx + 1) } else { None }
    });

    if let Some(line_no) = write_line {
        let touches_critical = content.contains(".bak")
            || content.contains("profiles.json")
            || content.contains("accounts.json")
            || content.contains("config.json")
            || content.contains("save_data")
            || content.contains("save_config");
        let has_atomic = ["os.replace", "std::fs::rename", "renameSync", "NamedTemporaryFile", ".tmp", "atomic"]
            .iter()
            .any(|a| content.contains(a));
        if touches_critical && !has_atomic {
            findings.push(SafetyFinding {
                code: "PERSISTENCE_OVERWRITE_SAFETY",
                severity: SafetySeverity::Advisory,
                line: Some(line_no),
                message: "Persistence write touches critical config or `.bak` without atomic temp+rename or empty-payload guards; prefer narrow partial updates and atomic replace.".to_string(),
            });
        }
    }
}
