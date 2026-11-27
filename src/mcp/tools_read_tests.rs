use super::*;
use serde_json::json;
use crate::mcp::state::ServerState;

#[test]
fn test_context_read_start_and_end_line_range() {
    let temp_dir = std::env::temp_dir().join(format!("read_range_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_dir);
    let sample_file = temp_dir.join("sample.rs");

    let mut lines = Vec::new();
    for i in 1..=50 {
        lines.push(format!("fn line_test_{}() {{ let _val = {}; }}", i, i));
    }
    let _ = std::fs::write(&sample_file, lines.join("\n"));

    let mut state = ServerState::new();
    let res = handle_tool_call(
        "project_context",
        json!({
            "operation": "read",
            "relative_path": "sample.rs",
            "start_line": 10,
            "end_line": 15,
            "project_path": temp_dir.to_str().unwrap()
        }),
        &mut state,
    );

    assert!(res.is_ok());
    let text = res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();

    // Verify header metadata contains line range
    assert!(text.contains("(Lines 10-15 of 50)"), "Missing range header: {}", text);

    // Verify default output is raw lines (no L10: prefix to save tokens)
    assert!(text.contains("fn line_test_10()"), "Missing line_test_10: {}", text);
    assert!(!text.contains("L10: "), "Should not contain L10: by default: {}", text);
    assert!(!text.contains("L15: "), "Should not contain L15: by default: {}", text);

    // Verify line_numbers: true option adds line prefixes
    let res_with_ln = handle_tool_call(
        "project_context",
        json!({
            "operation": "read",
            "relative_path": "sample.rs",
            "start_line": 10,
            "end_line": 15,
            "line_numbers": true,
            "project_path": temp_dir.to_str().unwrap()
        }),
        &mut state,
    );
    assert!(res_with_ln.is_ok());
    let text_ln = res_with_ln.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(text_ln.contains("L10: fn line_test_10()"), "Missing L10 prefix with line_numbers=true: {}", text_ln);
    assert!(text_ln.contains("L15: fn line_test_15()"), "Missing L15 prefix with line_numbers=true: {}", text_ln);

    // Verify it did not include lines outside range
    assert!(!text_ln.contains("L9: "), "Should not contain L9: {}", text_ln);
    assert!(!text_ln.contains("L16: "), "Should not contain L16: {}", text_ln);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_context_read_python_indent_preservation_and_tag() {
    let temp_dir = std::env::temp_dir().join(format!("read_py_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_dir);
    let py_file = temp_dir.join("service.py");

    let py_code = r#"class UserService:
    def __init__(self):
        self.users = []

    def get_user(self, user_id):
        if not user_id:
            return None
        return next((u for u in self.users if u['id'] == user_id), None)
"#;
    let _ = std::fs::write(&py_file, py_code);

    let mut state = ServerState::new();
    let res = handle_tool_call(
        "project_context",
        json!({
            "operation": "read",
            "relative_path": "service.py",
            "project_path": temp_dir.to_str().unwrap()
        }),
        &mut state,
    );

    assert!(res.is_ok());
    let text = res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();

    // Verify language note present
    assert!(text.contains("Language Note"), "Missing indent-sensitive note: {}", text);
    assert!(text.contains("Indent-sensitive syntax (Python/YAML/Makefile)"), "Missing py note: {}", text);

    // Verify indentation preserved cleanly without line numbers
    assert!(text.contains("class UserService:"), "Missing UserService: {}", text);
    assert!(text.contains("    def __init__(self):"), "Indentation lost at def __init__: {}", text);
    assert!(text.contains("            return None"), "Indentation lost at return None: {}", text);
    assert!(!text.contains("L1: "), "Should not contain L1 prefix: {}", text);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_context_read_blocks_external_absolute_path_and_preserves_html_comments() {
    let base = std::env::temp_dir().join(format!("read_sec_test_{}", std::process::id()));
    let repo = base.join("repo");
    let external = base.join("external");
    let _ = std::fs::create_dir_all(&repo);
    let _ = std::fs::create_dir_all(&external);

    let html_file = repo.join("index.html");
    std::fs::write(&html_file, "<div><!-- keep me --></div>\n").unwrap();
    let secret_file = external.join("secret.txt");
    std::fs::write(&secret_file, "TOP_SECRET").unwrap();

    let mut state = ServerState::new();
    state.workspace_roots = vec![repo.to_string_lossy().to_string()];

    let ext_res = handle_tool_call(
        "project_context",
        json!({
            "operation": "read",
            "project_path": repo.to_str().unwrap(),
            "relative_path": secret_file.to_str().unwrap()
        }),
        &mut state,
    )
    .unwrap();
    let ext_text = ext_res["content"][0]["text"].as_str().unwrap();
    assert!(ext_text.contains("Security Error"));
    assert!(!ext_text.contains("TOP_SECRET"));

    let html_res = handle_tool_call(
        "project_context",
        json!({
            "operation": "read",
            "project_path": repo.to_str().unwrap(),
            "relative_path": "index.html"
        }),
        &mut state,
    )
    .unwrap();
    let html_text = html_res["content"][0]["text"].as_str().unwrap();
    assert!(html_text.contains("<!-- keep me -->"));

    // Non-existent external absolute path must be rejected when allow_cross_workspace=false
    let ext_missing = external.join("nonexistent_new.rs");
    let missing_res = crate::mcp::tools::helpers::validate_path_scoped(
        &repo,
        ext_missing.to_str().unwrap(),
        false,
    );
    assert!(missing_res.is_err());

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn test_read_single_relative_paths_missing_symbol_and_out_of_range() {
    let temp_dir = std::env::temp_dir().join(format!("read_h13_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    let mut big_lines = Vec::new();
    big_lines.push("use crate::other::ghost_sym;".to_string());
    big_lines.push("// comment mentioning ghost_sym".to_string());
    for i in 1..=350 {
        big_lines.push(format!("pub fn fn_{}() {{ let _v = {}; }}", i, i));
    }
    std::fs::write(temp_dir.join("big.rs"), big_lines.join("\n")).unwrap();

    let mut state = ServerState::new();
    // 1. 1-element relative_paths routes through handle_read, preserving start_line/end_line and AstResponseCache
    let r1 = handle_tool_call(
        "project_context",
        json!({
            "operation": "read",
            "project_path": temp_dir.to_str().unwrap(),
            "relative_paths": ["big.rs"],
            "start_line": 5,
            "end_line": 10
        }),
        &mut state,
    )
    .unwrap();
    let t1 = r1["content"][0]["text"].as_str().unwrap();
    assert!(t1.contains("# Bounded File Content: big.rs (Lines 5-10 of 352)"), "Got: {}", t1);

    let r2 = handle_tool_call(
        "project_context",
        json!({
            "operation": "read",
            "project_path": temp_dir.to_str().unwrap(),
            "relative_paths": ["big.rs"],
            "start_line": 5,
            "end_line": 10
        }),
        &mut state,
    )
    .unwrap();
    let t2 = r2["content"][0]["text"].as_str().unwrap();
    assert!(t2.contains("AST Response Cache Hit"), "Expected cache hit on 1-element relative_paths, got: {}", t2);

    // 2. Missing symbol must NOT print Target Symbol Extracted
    let missing = handle_tool_call(
        "project_context",
        json!({
            "operation": "read",
            "project_path": temp_dir.to_str().unwrap(),
            "relative_path": "big.rs",
            "target_symbol": "ghost_sym"
        }),
        &mut state,
    )
    .unwrap();
    let tm = missing["content"][0]["text"].as_str().unwrap();
    assert!(!tm.contains("Target Symbol Extracted"), "Got: {}", tm);
    assert!(tm.contains("Symbol Not Found: 'ghost_sym'"), "Got: {}", tm);

    // 3. Out-of-range and >300 line range cap warnings
    let oor = handle_tool_call(
        "project_context",
        json!({
            "operation": "read",
            "project_path": temp_dir.to_str().unwrap(),
            "relative_path": "big.rs",
            "start_line": 900,
            "end_line": 950
        }),
        &mut state,
    )
    .unwrap();
    assert!(oor["content"][0]["text"].as_str().unwrap().contains("out of range"));

    let capped = handle_tool_call(
        "project_context",
        json!({
            "operation": "read",
            "project_path": temp_dir.to_str().unwrap(),
            "relative_path": "big.rs",
            "start_line": 1,
            "end_line": 340
        }),
        &mut state,
    )
    .unwrap();
    assert!(capped["content"][0]["text"].as_str().unwrap().contains("was_capped=true"));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_ast_cache_boundary_invalidation_and_bounded_eviction() {
    use crate::mcp::tools::context_cache::{AstCacheKey, AstResponseCache};

    let temp_dir = std::env::temp_dir().join(format!("ast_cache_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(temp_dir.join("src")).unwrap();

    let mod_rs = temp_dir.join("src/mod.rs");
    let chmod_rs = temp_dir.join("src/chmod.rs");
    std::fs::write(&mod_rs, "pub fn m() {}").unwrap();
    std::fs::write(&chmod_rs, "pub fn c() {}").unwrap();

    let mut cache = AstResponseCache::new(10);
    let key_mod = AstCacheKey {
        path: mod_rs.to_string_lossy().replace('\\', "/"),
        target_symbol: None,
        view_mode: "auto".to_string(),
        start_line: None,
        end_line: None,
        show_line_numbers: false,
    };
    let key_chmod = AstCacheKey {
        path: chmod_rs.to_string_lossy().replace('\\', "/"),
        target_symbol: None,
        view_mode: "auto".to_string(),
        start_line: None,
        end_line: None,
        show_line_numbers: false,
    };

    cache.insert(key_mod.clone(), "mod body".to_string(), &mod_rs, 10);
    cache.insert(key_chmod.clone(), "chmod body".to_string(), &chmod_rs, 10);

    // Invalidating "mod.rs" must NOT evict "src/chmod.rs"
    cache.invalidate_file("mod.rs");
    assert!(cache.get(&key_mod, &mod_rs).is_none());
    assert!(cache.get(&key_chmod, &chmod_rs).is_some());

    // Bounded eviction should keep remaining entries rather than clearing all
    for i in 0..12 {
        let k = AstCacheKey {
            path: chmod_rs.to_string_lossy().replace('\\', "/"),
            target_symbol: Some(format!("sym_{}", i)),
            view_mode: "auto".to_string(),
            start_line: None,
            end_line: None,
            show_line_numbers: false,
        };
        cache.insert(k, format!("body {}", i), &chmod_rs, 5);
    }
    assert!(cache.len() >= 8 && cache.len() <= 10, "Expected bounded cache size, got {}", cache.len());

    let _ = std::fs::remove_dir_all(&temp_dir);
}
