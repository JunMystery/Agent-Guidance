//! AST Semantic Context Slicing (Zoom Read) engine.
//! Preserves 100% of types, structs, traits, imports, and constants while expanding
//! only the target symbol's body and collapsing all sibling function bodies.

use super::types::{AstLanguage, AstSymbol};
use super::engine::AstEngine;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ZoomSliceResult {
    pub sliced_content: String,
    pub original_lines: usize,
    pub sliced_lines: usize,
    pub folded_functions_count: usize,
    pub savings_percent: usize,
    pub target_found: bool,
}

pub fn generate_zoom_slice(
    content: &str,
    lang: AstLanguage,
    file_path: &str,
    target_symbol: &str,
) -> Option<ZoomSliceResult> {
    let symbols = AstEngine::extract_symbols(content, lang);
    generate_zoom_slice_from_symbols(content, lang, file_path, target_symbol, &symbols)
}

pub(crate) fn generate_zoom_slice_from_symbols(
    content: &str,
    lang: AstLanguage,
    file_path: &str,
    target_symbol: &str,
    symbols: &[AstSymbol],
) -> Option<ZoomSliceResult> {
    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();

    let clean_target = target_symbol.trim().trim_end_matches("()");

    // Identify target symbol
    let is_target = |s: &AstSymbol| -> bool {
        s.name == clean_target
            || s.name.ends_with(&format!("::{}", clean_target))
            || s.name.ends_with(&format!(".{}", clean_target))
            || s.name.split("::").last() == Some(clean_target)
    };

    let target_exists = symbols.iter().any(is_target);
    if !target_exists {
        return None;
    }

    let mut fn_symbols: Vec<&AstSymbol> = symbols
        .iter()
        .filter(|s| s.kind == "function" || s.kind == "method")
        .collect();
    fn_symbols.sort_by_key(|s| (s.start_line, s.end_line));

    let is_py = matches!(lang, AstLanguage::Python);
    let mut sliced = Vec::new();
    sliced.push(format!("// === AST Semantic Context Slice: '{}' (Focus: '{}') ===", file_path, clean_target));

    let mut cur_line = 1;
    let mut folded_count = 0;

    for sym in fn_symbols {
        if sym.start_line == 0 || sym.start_line > total_lines || sym.start_line < cur_line {
            continue;
        }

        // Emit all intermediate context (imports, structs, types, comments)
        while cur_line < sym.start_line && cur_line <= total_lines {
            sliced.push(lines[cur_line - 1].to_string());
            cur_line += 1;
        }

        if cur_line <= total_lines {
            let end_line = sym.end_line.min(total_lines);
            if is_target(sym) {
                // Expanded focus node
                sliced.push(format!("// >>> [ZOOM FOCUS: {} (L{}-L{})] >>>", sym.name, sym.start_line, end_line));
                for l in sym.start_line..=end_line {
                    sliced.push(lines[l - 1].to_string());
                }
                sliced.push(format!("// <<< [END ZOOM FOCUS: {}] <<<", sym.name));
            } else if end_line > sym.start_line + 1 {
                // Fold sibling function body
                folded_count += 1;
                let start_idx = sym.start_line - 1;
                let first_line = lines[start_idx];
                let span = end_line.saturating_sub(sym.start_line);

                if is_py {
                    sliced.push(first_line.to_string());
                    let indent: String = first_line.chars().take_while(|c| c.is_whitespace()).collect();
                    let body_indent = if indent.is_empty() { "    ".to_string() } else { format!("{}    ", indent) };
                    sliced.push(format!("{}... /* L{}-{}: {} lines folded */", body_indent, sym.start_line + 1, end_line, span));
                } else {
                    let indent: String = first_line.chars().take_while(|c| c.is_whitespace()).collect();
                    let trimmed = first_line.trim();
                    if trimmed.ends_with('{') {
                        let header = trimmed.trim_end_matches('{').trim_end();
                        sliced.push(format!("{}{} {{ /* L{}-{}: {} lines folded */ }}", indent, header, sym.start_line + 1, end_line, span));
                    } else {
                        sliced.push(first_line.to_string());
                        sliced.push(format!("{}{{ /* L{}-{}: {} lines folded */ }}", indent, sym.start_line + 1, end_line, span));
                    }
                }
            } else {
                // Single-line function
                for l in sym.start_line..=end_line {
                    sliced.push(lines[l - 1].to_string());
                }
            }
            cur_line = end_line + 1;
        }
    }

    // Emit remaining lines
    while cur_line <= total_lines {
        sliced.push(lines[cur_line - 1].to_string());
        cur_line += 1;
    }

    let sliced_content = sliced.join("\n");
    let sliced_lines = sliced.len();
    let savings_percent = if total_lines > sliced_lines {
        ((total_lines - sliced_lines) * 100) / total_lines
    } else {
        0
    };

    Some(ZoomSliceResult {
        sliced_content,
        original_lines: total_lines,
        sliced_lines,
        folded_functions_count: folded_count,
        savings_percent,
        target_found: true,
    })
}

#[cfg(test)]
#[path = "zoom_tests.rs"]
mod tests;

#[cfg(test)]
mod ordering_tests {
    use super::*;

    fn mk_fn(name: &str, start_line: usize, end_line: usize) -> AstSymbol {
        AstSymbol {
            name: name.to_string(),
            kind: "function".to_string(),
            start_line,
            end_line,
            start_byte: 0,
            end_byte: 0,
            signature: format!("fn {}()", name),
            body_start_line: None,
            body_end_line: None,
            parent_symbol: None,
            package: None,
            language: "rust".to_string(),
        }
    }

    #[test]
    fn test_zoom_slice_sorts_out_of_order_and_guards_invalid_lines() {
        let code = "\
fn target_fn() {
    let x = 10;
    let y = 20;
    x + y
}

fn sibling_fn() {
    let a = 1;
    let b = 2;
    a + b
}";
        // Pass sibling BEFORE target, plus invalid 0 and out-of-bounds line numbers
        let symbols = vec![
            mk_fn("sibling_fn", 7, 11),
            mk_fn("zero_line", 0, 2),
            mk_fn("oob_line", 200, 210),
            mk_fn("target_fn", 1, 5),
        ];

        let res = generate_zoom_slice_from_symbols(code, AstLanguage::Rust, "demo.rs", "target_fn", &symbols)
            .expect("zoom slice should succeed");
        assert!(res.target_found);
        assert_eq!(res.folded_functions_count, 1);
        assert!(res.sliced_content.contains("ZOOM FOCUS: target_fn"));
        assert!(res.sliced_content.contains("let x = 10;"));
        assert!(res.sliced_content.contains("fn sibling_fn() { /* L8-11: 4 lines folded */ }"));
        assert!(!res.sliced_content.contains("let a = 1;"));
    }
}
