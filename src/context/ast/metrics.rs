//! Static code quality and AST complexity analyzer (Function LOC, Cyclomatic, Cognitive, Nesting).

use std::path::Path;

use super::engine::AstEngine;
use super::metrics_heuristic::analyze_heuristic_functions;
use super::metrics_walker::collect_tier1_functions;
use super::types::AstLanguage;

pub const SOFT_FILE_LOC_LIMIT: usize = 300;
pub const MAX_FUNCTION_LOC: usize = 60;
pub const MAX_CYCLOMATIC: usize = 10;
pub const MAX_COGNITIVE: usize = 15;
pub const MAX_NESTING_DEPTH: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionMetrics {
    pub name: String,
    pub start_line: usize,
    pub end_line: usize,
    pub loc: usize,
    pub cyclomatic: usize,
    pub cognitive: usize,
    pub max_nesting: usize,
}

impl FunctionMetrics {
    pub fn is_flagged(&self) -> bool {
        self.loc > MAX_FUNCTION_LOC
            || self.cyclomatic > MAX_CYCLOMATIC
            || self.cognitive > MAX_COGNITIVE
            || self.max_nesting > MAX_NESTING_DEPTH
    }

    fn severity_score(&self) -> usize {
        self.cognitive + self.cyclomatic + (self.loc / 4) + (self.max_nesting * 5)
    }
}

#[derive(Debug, Clone, Default)]
pub struct FileQualityReport {
    pub total_lines: usize,
    pub flagged_functions: Vec<FunctionMetrics>,
    pub file_loc_exceeded: bool,
}

impl FileQualityReport {
    pub fn has_warnings(&self) -> bool {
        self.file_loc_exceeded || !self.flagged_functions.is_empty()
    }

    pub fn format_advisory(&self, rel_path: &str) -> Option<String> {
        if !self.has_warnings() {
            return None;
        }
        let mut lines = Vec::new();
        if self.file_loc_exceeded {
            lines.push(format!(
                "- Soft File LOC Notice: `{}` has **{} lines** (soft limit: {} lines). Review question: *Is this file handling unrelated responsibilities?* If cohesive within a single domain/use case, keeping it intact is reasonable.",
                rel_path, self.total_lines, SOFT_FILE_LOC_LIMIT
            ));
        }
        for fn_m in self.flagged_functions.iter().take(5) {
            lines.push(format!(
                "- Function `{}` (L{}-{}): {} LOC (target 15-40, warn >{}), cyclomatic={} (<={}), cognitive={} (<={}), nesting={} (<={}). Consider extracting sub-steps into focused helpers.",
                fn_m.name,
                fn_m.start_line,
                fn_m.end_line,
                fn_m.loc,
                MAX_FUNCTION_LOC,
                fn_m.cyclomatic,
                MAX_CYCLOMATIC,
                fn_m.cognitive,
                MAX_COGNITIVE,
                fn_m.max_nesting,
                MAX_NESTING_DEPTH
            ));
        }
        if self.flagged_functions.len() > 5 {
            lines.push(format!(
                "- ...and {} more complex function(s) in `{}`.",
                self.flagged_functions.len() - 5,
                rel_path
            ));
        }
        Some(lines.join("\n"))
    }
}

/// Returns true for dedicated test files so long linear test cases do not trigger function-size noise.
pub fn is_test_file_path(rel_path: &str) -> bool {
    let lower = rel_path.trim().replace('\\', "/").to_lowercase();
    let file_name = Path::new(&lower)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    lower.starts_with("tests/")
        || lower.contains("/tests/")
        || lower.contains("/__tests__/")
        || lower.ends_with("_test.rs")
        || lower.ends_with("_tests.rs")
        || lower.ends_with(".test.ts")
        || lower.ends_with(".spec.ts")
        || lower.ends_with(".test.tsx")
        || lower.ends_with(".spec.tsx")
        || lower.ends_with(".test.js")
        || lower.ends_with(".spec.js")
        || lower.ends_with("_test.go")
        || lower.ends_with("_test.py")
        || (file_name.starts_with("test_") && file_name.ends_with(".py"))
}

pub fn analyze_file_path(full_path: &Path, rel_path: &str) -> Option<FileQualityReport> {
    if !full_path.exists() || !full_path.is_file() {
        return None;
    }
    let content = std::fs::read_to_string(full_path).ok()?;
    Some(analyze_file_quality(&content, rel_path))
}

pub fn analyze_file_quality(content: &str, rel_path: &str) -> FileQualityReport {
    if crate::mcp::tools::gate_exemptions::is_exempt_from_loc_limit(rel_path)
        || is_test_file_path(rel_path)
    {
        return FileQualityReport::default();
    }

    let total_lines = content.lines().count();
    let lang = AstLanguage::from_path(rel_path);
    let functions = if lang.is_tier1() {
        AstEngine::parse(content, lang)
            .map(|tree| collect_tier1_functions(tree.root_node(), content.as_bytes()))
            .unwrap_or_default()
    } else if lang.is_supported() {
        analyze_heuristic_functions(content, lang)
    } else {
        Vec::new()
    };

    let mut flagged: Vec<FunctionMetrics> = functions.into_iter().filter(|f| f.is_flagged()).collect();
    flagged.sort_by(|a, b| b.severity_score().cmp(&a.severity_score()));

    FileQualityReport {
        total_lines,
        flagged_functions: flagged,
        file_loc_exceeded: total_lines > SOFT_FILE_LOC_LIMIT,
    }
}
