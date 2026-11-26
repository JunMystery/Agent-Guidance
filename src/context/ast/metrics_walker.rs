//! Tree-Sitter Tier 1 AST walker for function complexity metrics.

use tree_sitter::Node;

use super::metrics::FunctionMetrics;

pub(super) fn collect_tier1_functions(root: Node, source: &[u8]) -> Vec<FunctionMetrics> {
    let mut out = Vec::new();
    walk_for_functions(root, source, &mut out);
    out
}

fn is_function_kind(kind: &str) -> bool {
    matches!(
        kind,
        "function_item"
            | "function_definition"
            | "function_declaration"
            | "generator_function_declaration"
            | "method_definition"
            | "method_declaration"
    )
}

fn walk_for_functions(node: Node, source: &[u8], out: &mut Vec<FunctionMetrics>) {
    let mut cursor = node.walk();
    let mut skip_cfg_test_mod = false;
    let mut skip_test_fn = false;

    for child in node.children(&mut cursor) {
        let kind = child.kind();
        if kind == "attribute_item" {
            let attr = std::str::from_utf8(&source[child.start_byte()..child.end_byte()]).unwrap_or("");
            if attr.contains("cfg(test)") {
                skip_cfg_test_mod = true;
            }
            if attr.contains("#[test]") || attr.contains("::test]") || attr.contains("::test(") {
                skip_test_fn = true;
            }
            continue;
        }

        if kind == "mod_item" && skip_cfg_test_mod {
            skip_cfg_test_mod = false;
            skip_test_fn = false;
            continue;
        }

        if is_function_kind(kind) {
            if !skip_test_fn {
                out.push(measure_function_node(child, source));
            }
            skip_cfg_test_mod = false;
            skip_test_fn = false;
            continue;
        }

        skip_cfg_test_mod = false;
        skip_test_fn = false;
        walk_for_functions(child, source, out);
    }
}

fn measure_function_node(node: Node, source: &[u8]) -> FunctionMetrics {
    let name = node
        .child_by_field_name("name")
        .and_then(|n| std::str::from_utf8(&source[n.start_byte()..n.end_byte()]).ok())
        .unwrap_or("<anonymous>")
        .to_string();
    let start_line = node.start_position().row + 1;
    let end_line = node.end_position().row + 1;
    let loc = end_line.saturating_sub(start_line) + 1;

    let mut acc = ComplexityAcc {
        cyclomatic: 1,
        cognitive: 0,
        max_nesting: 0,
    };
    if let Some(body) = node.child_by_field_name("body") {
        walk_complexity(body, source, 0, &mut acc);
    } else {
        walk_complexity(node, source, 0, &mut acc);
    }

    FunctionMetrics {
        name,
        start_line,
        end_line,
        loc,
        cyclomatic: acc.cyclomatic,
        cognitive: acc.cognitive,
        max_nesting: acc.max_nesting,
    }
}

struct ComplexityAcc {
    cyclomatic: usize,
    cognitive: usize,
    max_nesting: usize,
}

fn walk_complexity(node: Node, source: &[u8], nesting: usize, acc: &mut ComplexityAcc) {
    let kind = node.kind();

    if matches!(kind, "if_expression" | "if_statement") {
        let is_else_if = is_else_if_node(&node);
        acc.cyclomatic += 1;
        acc.cognitive += if is_else_if { 1 } else { 1 + nesting };
        acc.max_nesting = acc.max_nesting.max(nesting + 1);
        walk_if_children(node, source, nesting, acc);
        return;
    }

    if kind == "else_clause" {
        if else_clause_has_if(node) {
            walk_children_at(node, source, nesting, acc);
        } else {
            acc.cognitive += 1;
            acc.max_nesting = acc.max_nesting.max(nesting + 1);
            walk_children_at(node, source, nesting + 1, acc);
        }
        return;
    }

    if kind == "elif_clause" {
        acc.cyclomatic += 1;
        acc.cognitive += 1;
        acc.max_nesting = acc.max_nesting.max(nesting + 1);
        let cond_id = node.child_by_field_name("condition").map(|n| n.id());
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            let depth = if Some(child.id()) == cond_id { nesting } else { nesting + 1 };
            walk_complexity(child, source, depth, acc);
        }
        return;
    }

    if is_branch_or_loop_kind(kind) {
        acc.cyclomatic += 1;
        acc.cognitive += 1 + nesting;
        acc.max_nesting = acc.max_nesting.max(nesting + 1);
        walk_children_at(node, source, nesting + 1, acc);
        return;
    }

    if matches!(kind, "binary_expression" | "boolean_operator") {
        if let Some(op) = logical_op(&node, source) {
            acc.cyclomatic += 1;
            if !same_parent_logical_op(&node, source, op) {
                acc.cognitive += 1;
            }
        }
    }

    walk_children_at(node, source, nesting, acc);
}

fn walk_if_children(node: Node, source: &[u8], nesting: usize, acc: &mut ComplexityAcc) {
    let cond_id = node.child_by_field_name("condition").map(|n| n.id());
    let alt_id = node.child_by_field_name("alternative").map(|n| n.id());
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let k = child.kind();
        if Some(child.id()) == cond_id {
            walk_complexity(child, source, nesting, acc);
        } else if k == "else_clause" || k == "elif_clause" || Some(child.id()) == alt_id {
            if Some(child.id()) == alt_id && !matches!(k, "else_clause" | "elif_clause" | "if_statement" | "if_expression") {
                acc.cognitive += 1;
                acc.max_nesting = acc.max_nesting.max(nesting + 1);
                walk_children_at(child, source, nesting + 1, acc);
            } else {
                walk_complexity(child, source, nesting, acc);
            }
        } else {
            walk_complexity(child, source, nesting + 1, acc);
        }
    }
}

fn walk_children_at(node: Node, source: &[u8], nesting: usize, acc: &mut ComplexityAcc) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_complexity(child, source, nesting, acc);
    }
}

fn else_clause_has_if(node: Node) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .any(|c| matches!(c.kind(), "if_expression" | "if_statement"))
}

fn is_else_if_node(node: &Node) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    if parent.kind() == "else_clause" {
        return true;
    }
    matches!(parent.kind(), "if_statement" | "if_expression")
        && parent
            .child_by_field_name("alternative")
            .is_some_and(|alt| alt.id() == node.id())
}

fn is_branch_or_loop_kind(kind: &str) -> bool {
    matches!(
        kind,
        "for_expression"
            | "for_statement"
            | "for_in_statement"
            | "while_expression"
            | "while_statement"
            | "loop_expression"
            | "do_statement"
            | "match_expression"
            | "match_statement"
            | "switch_statement"
            | "expression_switch_statement"
            | "type_switch_statement"
            | "select_statement"
            | "catch_clause"
            | "except_clause"
            | "conditional_expression"
            | "ternary_expression"
    )
}

fn logical_op<'a>(node: &Node<'a>, source: &'a [u8]) -> Option<&'a str> {
    let op_node = node.child_by_field_name("operator")?;
    let text = std::str::from_utf8(&source[op_node.start_byte()..op_node.end_byte()]).ok()?.trim();
    match text {
        "&&" | "||" | "and" | "or" => Some(text),
        _ => None,
    }
}

fn same_parent_logical_op(node: &Node, source: &[u8], op: &str) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    matches!(parent.kind(), "binary_expression" | "boolean_operator")
        && logical_op(&parent, source) == Some(op)
}
