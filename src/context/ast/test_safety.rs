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

pub fn analyze_verification_command(cmd: &str) -> SafetyReport {
    let lower = cmd.to_lowercase();
    let has_safe_override = lower.contains(":memory:")
        || lower.contains("--dry-run")
        || lower.contains("test_db")
        || lower.contains("test-safety-allow:");
    if has_safe_override {
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
