//! Unit tests for AST complexity metrics, anti-noise rules, and test-file exemptions.

use super::metrics::{analyze_file_quality, is_test_file_path};

#[test]
fn test_clean_340_line_file_triggers_only_soft_loc_notice() {
    let mut lines = Vec::new();
    for i in 1..=20 {
        lines.push(format!("pub fn step_{}(x: i32) -> i32 {{", i));
        for j in 1..=14 {
            lines.push(format!("    let v_{} = x + {};", j, j));
        }
        lines.push("    v_14".to_string());
        lines.push("}".to_string());
    }
    let content = lines.join("\n");
    assert!(content.lines().count() > 300);

    let report = analyze_file_quality(&content, "src/domain/order_steps.rs");
    assert!(report.file_loc_exceeded);
    assert!(report.flagged_functions.is_empty(), "Small clean functions must not be flagged: {:?}", report.flagged_functions);
    let adv = report.format_advisory("src/domain/order_steps.rs").unwrap();
    assert!(adv.contains("Soft File LOC Notice"));
    assert!(adv.contains("unrelated responsibilities"));
    assert!(!adv.contains("Function `"));
}

#[test]
fn test_flat_35_arm_match_is_not_flagged_as_complex() {
    let mut lines = vec!["pub fn map_code(code: u32) -> &'static str {".to_string(), "    match code {".to_string()];
    for i in 1..=35 {
        lines.push(format!("        {} => \"val_{}\",", i, i));
    }
    lines.push("        _ => \"other\",".to_string());
    lines.push("    }".to_string());
    lines.push("}".to_string());

    let report = analyze_file_quality(&lines.join("\n"), "src/domain/codes.rs");
    assert!(!report.file_loc_exceeded);
    assert!(report.flagged_functions.is_empty(), "Flat match lookup should not trigger false positive: {:?}", report.flagged_functions);
    assert!(report.format_advisory("src/domain/codes.rs").is_none());
}

#[test]
fn test_long_deeply_nested_function_in_small_file_is_flagged() {
    let mut lines = vec![
        "pub fn process_order(a: i32, b: i32, c: i32, d: i32, e: i32) -> i32 {".to_string(),
        "    if a > 0 {".to_string(),
        "        for i in 0..b {".to_string(),
        "            if i > c {".to_string(),
        "                while d > 0 {".to_string(),
        "                    if e > 1 && a < 10 || b > 2 {".to_string(),
        "                        return 1;".to_string(),
        "                    }".to_string(),
        "                    break;".to_string(),
        "                }".to_string(),
        "            }".to_string(),
        "        }".to_string(),
        "    }".to_string(),
    ];
    for i in 1..=55 {
        lines.push(format!("    let pad_{} = {};", i, i));
    }
    lines.push("    0".to_string());
    lines.push("}".to_string());

    let content = lines.join("\n");
    assert!(content.lines().count() < 100);

    let report = analyze_file_quality(&content, "src/orders/processor.rs");
    assert!(!report.file_loc_exceeded);
    assert_eq!(report.flagged_functions.len(), 1);
    let fn_m = &report.flagged_functions[0];
    assert_eq!(fn_m.name, "process_order");
    assert!(fn_m.loc > 60);
    assert!(fn_m.max_nesting >= 5);
    assert!(fn_m.cognitive > 15);

    let adv = report.format_advisory("src/orders/processor.rs").unwrap();
    assert!(adv.contains("Function `process_order`"));
    assert!(adv.contains("nesting="));
    assert!(!adv.contains("Soft File LOC Notice"));
}

#[test]
fn test_inline_cfg_test_and_test_file_paths_are_exempt() {
    assert!(is_test_file_path("src/mcp/tools_gate_tests.rs"));
    assert!(is_test_file_path("tests/integration_test.rs"));
    assert!(is_test_file_path("frontend/src/App.spec.ts"));
    assert!(is_test_file_path("backend/tests/test_orders.py"));
    assert!(!is_test_file_path("src/orders/handler.rs"));

    let mut lines = vec![
        "pub fn ok() -> i32 { 1 }".to_string(),
        "#[cfg(test)]".to_string(),
        "mod tests {".to_string(),
        "    #[test]".to_string(),
        "    fn long_test() {".to_string(),
    ];
    for i in 1..=70 {
        lines.push(format!("        let x_{} = {};", i, i));
    }
    lines.push("    }".to_string());
    lines.push("}".to_string());

    let report = analyze_file_quality(&lines.join("\n"), "src/orders/handler.rs");
    assert!(!report.has_warnings());
}

#[test]
fn test_tier2_kotlin_heuristic_flags_deep_nesting() {
    let content = r#"
class OrderProcessor {
    fun handle(a: Int, b: Int, c: Int, d: Int, e: Int): Int {
        if (a > 0) {
            for (i in 0..b) {
                if (i > c) {
                    while (d > 0) {
                        if (e > 1) {
                            return 1
                        }
                    }
                }
            }
        }
        return 0
    }
}
"#;
    let report = analyze_file_quality(content, "app/OrderProcessor.kt");
    assert_eq!(report.flagged_functions.len(), 1);
    assert_eq!(report.flagged_functions[0].name, "handle");
    assert!(report.flagged_functions[0].max_nesting >= 5);
}

#[test]
fn test_flat_else_if_chain_does_not_inflate_nesting() {
    let content = r#"
pub fn classify(x: i32) -> i32 {
    if x == 1 {
        10
    } else if x == 2 {
        20
    } else if x == 3 {
        30
    } else if x == 4 {
        40
    } else {
        0
    }
}
"#;
    let report = analyze_file_quality(content, "src/domain/classifier.rs");
    assert!(!report.has_warnings());
    let tree = super::engine::AstEngine::parse(content, super::types::AstLanguage::Rust).unwrap();
    let fns = super::metrics_walker::collect_tier1_functions(tree.root_node(), content.as_bytes());
    assert_eq!(fns.len(), 1);
    assert_eq!(fns[0].max_nesting, 1);
    assert_eq!(fns[0].cognitive, 5);
}
