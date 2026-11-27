//! Extract call, import, and structural containment edges from file content.

use super::parsers::{ExtractedEdge, ExtractedSymbol};

/// Extract call edges and import edges for a file
pub fn extract_edges_from_content(
    rel_path: &str,
    content: &str,
    symbols: &[ExtractedSymbol],
) -> Vec<ExtractedEdge> {
    // If it's a doc or structured data file, do not extract code call/member edges
    if super::doc_data::classify_doc_or_data(rel_path).is_some() {
        return Vec::new();
    }

    let mut edges = Vec::new();
    let mod_id = super::resolver::find_module_symbol(symbols)
        .map(|s| s.id.as_str())
        .unwrap_or("");

    // 1. Imports extraction
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("use ") || trimmed.starts_with("import ") || trimmed.starts_with("from ") {
            let token = trimmed
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .filter(|s| !s.is_empty() && *s != "use" && *s != "import" && *s != "from" && *s != "crate" && *s != "super")
                .last();
            if let Some(target_name) = token {
                if let Some(target) = super::resolver::resolve_local_callee(symbols, target_name, mod_id) {
                    if !mod_id.is_empty() && mod_id != target.id {
                        edges.push(ExtractedEdge {
                            source_id: mod_id.to_string(),
                            target_id: target.id.clone(),
                            edge_type: "imports".to_string(),
                            weight: 1.0,
                            confidence: 0.95,
                            category: "file".to_string(),
                            call_line: None,
                        });
                    }
                }
            }
        }
    }

    // 2. Call edges: AST-native when supported, heuristic fallback otherwise
    let lang = crate::context::ast::AstLanguage::from_path(rel_path);
    if lang.is_supported() {
        let ast_calls = crate::context::ast::AstEngine::extract_calls(content, lang);
        for call in ast_calls {
            let caller = super::resolver::find_enclosing_symbol(symbols, call.line);
            let caller_id = caller.map(|c| c.id.as_str()).unwrap_or(mod_id);
            if caller_id.is_empty() {
                continue;
            }

            if let Some(target) = super::resolver::resolve_local_callee(symbols, &call.callee_name, caller_id) {
                edges.push(ExtractedEdge {
                    source_id: caller_id.to_string(),
                    target_id: target.id.clone(),
                    edge_type: "calls".to_string(),
                    weight: 1.0,
                    confidence: 1.0,
                    category: "symbol".to_string(),
                    call_line: Some(call.line),
                });
            }
        }
    } else {
        for (idx, line) in content.lines().enumerate() {
            let line_num = idx + 1;
            let caller = super::resolver::find_enclosing_symbol(symbols, line_num);
            let caller_id = caller.map(|c| c.id.as_str()).unwrap_or(mod_id);
            if caller_id.is_empty() {
                continue;
            }

            for sym in symbols {
                if sym.kind != "module" && sym.kind != "package" && sym.id != caller_id && line.contains(&format!("{}(", sym.name)) {
                    edges.push(ExtractedEdge {
                        source_id: caller_id.to_string(),
                        target_id: sym.id.clone(),
                        edge_type: "calls".to_string(),
                        weight: 1.0,
                        confidence: 0.70,
                        category: "symbol".to_string(),
                        call_line: Some(line_num),
                    });
                }
            }
        }
    }

    // 3. Structural containment edges
    for sym in symbols {
        if sym.kind != "module" && sym.kind != "package" {
            if let Some(parent_name) = &sym.parent {
                if let Some(parent_sym) = symbols.iter().find(|s| &s.name == parent_name && s.id != sym.id) {
                    edges.push(ExtractedEdge {
                        source_id: parent_sym.id.clone(),
                        target_id: sym.id.clone(),
                        edge_type: "member_of".to_string(),
                        weight: 1.0,
                        confidence: 1.0,
                        category: "structural".to_string(),
                        call_line: None,
                    });
                }
            } else if !mod_id.is_empty() {
                edges.push(ExtractedEdge {
                    source_id: mod_id.to_string(),
                    target_id: sym.id.clone(),
                    edge_type: "contains".to_string(),
                    weight: 1.0,
                    confidence: 1.0,
                    category: "structural".to_string(),
                    call_line: None,
                });
            }
        }
    }

    edges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_member_of_skips_self_loop_for_same_named_constructor() {
        let symbols = vec![
            ExtractedSymbol {
                id: "src/foo.java::method::Foo::L5".to_string(),
                name: "Foo".to_string(),
                kind: "method".to_string(),
                parent: Some("Foo".to_string()),
                start_line: 5,
                end_line: 8,
                signature: Some("Foo()".to_string()),
                language: Some("java".to_string()),
                namespace: None,
                receiver: None,
            },
            ExtractedSymbol {
                id: "src/foo.java::class::Foo::L1".to_string(),
                name: "Foo".to_string(),
                kind: "class".to_string(),
                parent: None,
                start_line: 1,
                end_line: 10,
                signature: Some("class Foo".to_string()),
                language: Some("java".to_string()),
                namespace: None,
                receiver: None,
            },
        ];

        let edges = extract_edges_from_content("src/foo.java", "class Foo { Foo() {} }", &symbols);
        let member_edges: Vec<&ExtractedEdge> = edges.iter().filter(|e| e.edge_type == "member_of").collect();
        assert_eq!(member_edges.len(), 1);
        assert_eq!(member_edges[0].source_id, "src/foo.java::class::Foo::L1");
        assert_eq!(member_edges[0].target_id, "src/foo.java::method::Foo::L5");
    }
}
