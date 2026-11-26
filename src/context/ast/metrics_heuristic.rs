//! Heuristic fallback complexity analyzer for Tier 2 polyglot languages.

use super::engine::AstEngine;
use super::metrics::FunctionMetrics;
use super::types::AstLanguage;

pub(super) fn analyze_heuristic_functions(content: &str, lang: AstLanguage) -> Vec<FunctionMetrics> {
    let lines: Vec<&str> = content.lines().collect();
    let symbols = AstEngine::extract_symbols(content, lang);
    let mut out = Vec::new();

    for sym in symbols {
        if sym.kind != "function" && sym.kind != "method" {
            continue;
        }
        if sym.start_line == 0 || sym.end_line < sym.start_line || sym.end_line > lines.len() {
            continue;
        }
        let slice = &lines[sym.start_line - 1..sym.end_line];
        let loc = sym.end_line - sym.start_line + 1;
        let (cyclomatic, cognitive, max_nesting) = measure_slice_complexity(slice);
        out.push(FunctionMetrics {
            name: sym.name,
            start_line: sym.start_line,
            end_line: sym.end_line,
            loc,
            cyclomatic,
            cognitive,
            max_nesting,
        });
    }
    out
}

fn measure_slice_complexity(slice: &[&str]) -> (usize, usize, usize) {
    let mut cyclomatic = 1usize;
    let mut cognitive = 0usize;
    let mut max_nesting = 0usize;
    let mut brace_depth = 0usize;
    let mut control_stack: Vec<usize> = Vec::new();

    for (idx, raw_line) in slice.iter().enumerate() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') || trimmed.starts_with("/*") {
            continue;
        }

        let close_braces = trimmed.matches('}').count();
        brace_depth = brace_depth.saturating_sub(close_braces);
        while control_stack.last().is_some_and(|&d| brace_depth < d) {
            control_stack.pop();
        }

        let nesting = control_stack.len();
        let is_else_if = trimmed.starts_with("else if") || trimmed.starts_with("} else if") || trimmed.starts_with("elif ");
        let is_plain_else = (trimmed.starts_with("else") || trimmed.starts_with("} else")) && !is_else_if;

        if is_else_if {
            cyclomatic += 1;
            cognitive += 1;
            max_nesting = max_nesting.max(nesting + 1);
            if idx > 0 && trimmed.contains('{') {
                control_stack.push(brace_depth + 1);
            }
        } else if is_plain_else {
            cognitive += 1;
            max_nesting = max_nesting.max(nesting + 1);
            if idx > 0 && trimmed.contains('{') {
                control_stack.push(brace_depth + 1);
            }
        } else if has_control_keyword(trimmed) {
            cyclomatic += 1;
            cognitive += 1 + nesting;
            max_nesting = max_nesting.max(nesting + 1);
            if idx > 0 && trimmed.contains('{') {
                control_stack.push(brace_depth + 1);
            }
        }

        let bool_ops = trimmed.matches("&&").count() + trimmed.matches("||").count();
        cyclomatic += bool_ops;
        if bool_ops > 0 {
            cognitive += 1;
        }

        let open_braces = trimmed.matches('{').count();
        brace_depth += open_braces;
    }

    (cyclomatic, cognitive, max_nesting)
}

fn has_control_keyword(trimmed: &str) -> bool {
    const KEYWORDS: [&str; 8] = ["if ", "if(", "for ", "for(", "while ", "while(", "when ", "switch "];
    KEYWORDS.iter().any(|kw| trimmed.starts_with(kw) || trimmed.contains(&format!(" {}", kw)))
        || trimmed.starts_with("catch ")
        || trimmed.starts_with("catch(")
}
