//! AST Engine providing Tree-Sitter parsing and analysis capabilities.

use tree_sitter::{Language, Parser, Tree};
use super::types::{AstCall, AstLanguage, AstSymbol};
use super::walker;

pub struct AstEngine;

impl AstEngine {
    /// Returns the tree-sitter Language for the given AstLanguage.
    pub fn get_language(lang: AstLanguage) -> Option<Language> {
        match lang {
            AstLanguage::Rust => Some(tree_sitter_rust::LANGUAGE.into()),
            AstLanguage::TypeScript => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
            AstLanguage::Tsx | AstLanguage::JavaScript => Some(tree_sitter_typescript::LANGUAGE_TSX.into()),
            AstLanguage::Python => Some(tree_sitter_python::LANGUAGE.into()),
            AstLanguage::Go => Some(tree_sitter_go::LANGUAGE.into()),
            _ => None,
        }
    }

    /// Parses source code using the appropriate Tree-Sitter grammar.
    pub fn parse(content: &str, lang: AstLanguage) -> Option<Tree> {
        let language = Self::get_language(lang)?;
        let mut parser = Parser::new();
        parser.set_language(&language).ok()?;
        parser.parse(content, None)
    }

    /// Extracts all symbols (functions, structs, classes, methods) via AST analysis.
    pub fn extract_symbols(content: &str, lang: AstLanguage) -> Vec<AstSymbol> {
        if lang.is_tier1() {
            if let Some(tree) = Self::parse(content, lang) {
                walker::extract_symbols(tree.root_node(), content.as_bytes(), lang)
            } else {
                Vec::new()
            }
        } else if lang.is_tier2() {
            super::polyglot::extract_symbols(content, lang)
        } else if lang.is_frontend() {
            super::frontend::extract_symbols(content, lang)
        } else if lang.is_database() {
            super::database::extract_symbols(content, lang)
        } else if lang.is_manifest() {
            super::manifests::extract_symbols(content, lang)
        } else {
            Vec::new()
        }
    }

    /// Extracts all function calls via AST analysis.
    pub fn extract_calls(content: &str, lang: AstLanguage) -> Vec<AstCall> {
        if lang.is_tier1() {
            if let Some(tree) = Self::parse(content, lang) {
                walker::extract_calls(tree.root_node(), content.as_bytes(), lang)
            } else {
                Vec::new()
            }
        } else if lang.is_tier2() {
            super::polyglot::extract_calls(content, lang)
        } else if lang.is_frontend() {
            super::frontend::extract_calls(content, lang)
        } else if lang.is_database() {
            super::database::extract_calls(content, lang)
        } else if lang.is_manifest() {
            super::manifests::extract_calls(content, lang)
        } else {
            Vec::new()
        }
    }

    /// Finds a specific symbol by exact name or suffix match.
    pub fn find_symbol(content: &str, lang: AstLanguage, symbol_name: &str) -> Option<AstSymbol> {
        let symbols = Self::extract_symbols(content, lang);
        symbols
            .into_iter()
            .find(|s| s.name == symbol_name || s.name.ends_with(&format!("::{}", symbol_name)))
    }

    /// Generates an AST semantic context slice (Zoom Read) expanding only target_symbol and folding siblings.
    pub fn generate_zoom_slice(
        content: &str,
        lang: AstLanguage,
        file_path: &str,
        target_symbol: &str,
    ) -> Option<super::zoom::ZoomSliceResult> {
        super::zoom::generate_zoom_slice(content, lang, file_path, target_symbol)
    }

    /// Generates a concise AST-aware code skeleton with function bodies collapsed.
    pub fn generate_skeleton(content: &str, lang: AstLanguage, file_path: &str) -> Option<String> {
        let symbols = Self::extract_symbols(content, lang);
        Self::generate_skeleton_from_symbols(content, lang, file_path, &symbols)
    }

    pub(crate) fn generate_skeleton_from_symbols(
        content: &str,
        lang: AstLanguage,
        file_path: &str,
        symbols: &[AstSymbol],
    ) -> Option<String> {
        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        // Only collapse function and method bodies
        let mut fn_symbols: Vec<&AstSymbol> = symbols
            .iter()
            .filter(|s| (s.kind == "function" || s.kind == "method") && s.end_line > s.start_line + 1)
            .collect();
        fn_symbols.sort_by_key(|s| (s.start_line, s.end_line));

        if fn_symbols.is_empty() {
            return None;
        }

        let is_py = matches!(lang, AstLanguage::Python);
        let mut skeleton = Vec::new();
        skeleton.push(format!("// === AST Structural Skeleton of '{}' (Total Lines: {}) ===", file_path, total_lines));

        let mut cur_line = 1;
        for sym in fn_symbols {
            if sym.start_line == 0 || sym.start_line > total_lines || sym.start_line < cur_line {
                continue;
            }
            // Emit lines up to start_line
            while cur_line < sym.start_line && cur_line <= total_lines {
                skeleton.push(lines[cur_line - 1].to_string());
                cur_line += 1;
            }

            if cur_line <= total_lines {
                let start_line_idx = sym.start_line - 1;
                let first_line = lines[start_line_idx];
                let end_line = sym.end_line.min(total_lines);
                let span = end_line.saturating_sub(sym.start_line);

                if is_py {
                    skeleton.push(first_line.to_string());
                    let indent: String = first_line.chars().take_while(|c| c.is_whitespace()).collect();
                    let body_indent = if indent.is_empty() { "    ".to_string() } else { format!("{}    ", indent) };
                    skeleton.push(format!("{}... /* L{}-{}: {} lines */", body_indent, sym.start_line + 1, end_line, span));
                } else {
                    let indent: String = first_line.chars().take_while(|c| c.is_whitespace()).collect();
                    let trimmed = first_line.trim();
                    if trimmed.ends_with('{') {
                        let header = trimmed.trim_end_matches('{').trim_end();
                        skeleton.push(format!("{}{} {{ /* L{}-{}: {} lines */ }}", indent, header, sym.start_line + 1, end_line, span));
                    } else {
                        skeleton.push(first_line.to_string());
                        skeleton.push(format!("{}{{ /* L{}-{}: {} lines */ }}", indent, sym.start_line + 1, end_line, span));
                    }
                }
                cur_line = end_line + 1;
            }
        }

        // Emit remaining lines after last function
        while cur_line <= total_lines {
            skeleton.push(lines[cur_line - 1].to_string());
            cur_line += 1;
        }

        Some(skeleton.join("\n"))
    }
}

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
    fn test_generate_skeleton_sorts_out_of_order_and_guards_invalid_lines() {
        let code = "\
fn first() {
    let a = 1;
    let b = 2;
    a + b
}

fn second() {
    let c = 3;
    let d = 4;
    c + d
}";
        let symbols = vec![
            mk_fn("second", 7, 11),
            mk_fn("zero_line", 0, 3),
            mk_fn("oob_line", 99, 105),
            mk_fn("first", 1, 5),
        ];

        let skel = AstEngine::generate_skeleton_from_symbols(code, AstLanguage::Rust, "demo.rs", &symbols)
            .expect("skeleton should be generated");
        assert!(skel.contains("fn first() { /* L2-5: 4 lines */ }"));
        assert!(skel.contains("fn second() { /* L8-11: 4 lines */ }"));
        assert!(!skel.contains("let a = 1;"));
        assert!(!skel.contains("let c = 3;"));
    }
}
