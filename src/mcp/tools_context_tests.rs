use super::*;

#[test]
fn test_project_context_read_300_loc_warning() {
    let temp_dir = std::env::temp_dir().join(format!("read_loc_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_dir);
    let large_file = temp_dir.join("large_file.rs");
    let content = (1..=350).map(|i| format!("fn function_{}() {{}}", i)).collect::<Vec<_>>().join("\n");
    let _ = std::fs::write(&large_file, &content);

    let mut state = ServerState::new();
    let res = handle_tool_call(
        "project_context",
        json!({
            "operation": "read",
            "relative_path": "large_file.rs",
            "view_mode": "full",
            "project_path": temp_dir.to_str().unwrap()
        }),
        &mut state,
    );
    assert!(res.is_ok());
    let text = res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(text.contains("Code Quality Note (Soft Limit > 300 LOC)"));
    assert!(text.contains("350 total lines"));
    assert!(!text.contains("ARCHITECTURE MANDATE"));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_target_symbol_extraction_in_large_file() {
    let temp_dir = std::env::temp_dir().join(format!("symbol_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_dir);
    let large_file = temp_dir.join("giant_service.rs");

    // Create a 600-line file with a specific target function at line 450
    let mut lines = Vec::new();
    for i in 1..=440 {
        lines.push(format!("fn dummy_func_{}() {{ let _x = {}; }}", i, i));
    }
    lines.push("pub fn target_critical_function(user_id: u64) -> bool {".to_string());
    lines.push("    let is_valid = user_id > 1000;".to_string());
    lines.push("    println!(\"Processing user: {}\", user_id);".to_string());
    lines.push("    is_valid".to_string());
    lines.push("}".to_string());
    for i in 446..=600 {
        lines.push(format!("fn trailing_func_{}() {{ let _y = {}; }}", i, i));
    }
    let _ = std::fs::write(&large_file, lines.join("\n"));

    let mut state = ServerState::new();
    let res = handle_tool_call(
        "project_context",
        json!({
            "operation": "read",
            "relative_path": "giant_service.rs",
            "target_symbol": "target_critical_function",
            "project_path": temp_dir.to_str().unwrap()
        }),
        &mut state,
    );

    assert!(res.is_ok(), "Target symbol extraction failed: {:?}", res);
    let text = res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(text.contains("Target Symbol Extracted: 'target_critical_function'"), "Output missing target symbol header: {}", text);
    assert!(text.contains("pub fn target_critical_function(user_id: u64) -> bool"), "Output missing function signature: {}", text);
    assert!(text.contains("Processing user:"), "Output missing function body: {}", text);
    assert!(!text.contains("dummy_func_1"), "Output should NOT contain unrelated top functions: {}", text);
    assert!(!text.contains("trailing_func_500"), "Output should NOT contain unrelated bottom functions: {}", text);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_project_context_deep_search_and_snippets() {
    let temp_dir = std::env::temp_dir().join(format!("deep_search_test_{}", std::process::id()));
    let deep_path = temp_dir.join("app").join("src").join("main").join("java").join("com").join("example").join("ui").join("tour");
    let _ = std::fs::create_dir_all(&deep_path);
    let deep_file = deep_path.join("ProductTour.kt");
    let content = "package com.example.ui.tour\n\nclass ProductTour {\n    fun startTour() {\n        val tourAnchor = 42\n    }\n}\n";
    let _ = std::fs::write(&deep_file, content);

    let mut state = ServerState::new();
    let res = handle_tool_call(
        "project_context",
        json!({
            "operation": "search",
            "query": "touranchor",
            "project_path": temp_dir.to_str().unwrap()
        }),
        &mut state,
    );
    assert!(res.is_ok(), "Deep search failed: {:?}", res);
    let text = res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(text.contains("ProductTour.kt"), "Deep search did not find ProductTour.kt: {}", text);
    assert!(text.contains("ProductTour") || text.contains("tourAnchor"), "Deep search did not extract symbol/snippet: {}", text);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_project_context_kotlin_symbols() {
    let temp_dir = std::env::temp_dir().join(format!("kotlin_symbols_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_dir);
    let kt_file = temp_dir.join("ProductTour.kt");
    let content = "package com.nodescape.app.ui.tour\n\nclass ProductTourOverlay {\n    suspend fun showTour() {}\n    companion object {\n        fun create() {}\n    }\n}\n";
    let _ = std::fs::write(&kt_file, content);

    let mut state = ServerState::new();
    let res = handle_tool_call(
        "project_context",
        json!({
            "operation": "symbols",
            "relative_path": "ProductTour.kt",
            "project_path": temp_dir.to_str().unwrap()
        }),
        &mut state,
    );
    assert!(res.is_ok());
    let text = res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(text.contains("class ProductTourOverlay"));
    assert!(text.contains("suspend fun showTour"));
    assert!(text.contains("companion object"));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_project_context_cascade_and_learning() {
    let temp_dir = std::env::temp_dir().join(format!("ag_tools_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(temp_dir.join("src"));
    std::fs::write(
        temp_dir.join("src/payment.rs"),
        "pub struct PaymentGateway;\nimpl PaymentGateway {\n    pub fn charge_card(&self) {\n        let timeout = 30;\n    }\n}\n",
    ).unwrap();

    let mut state = ServerState::new();

    // 1. Reindex
    let reindex_res = handle_tool_call(
        "project_context",
        json!({
            "operation": "reindex",
            "project_path": temp_dir.to_str().unwrap()
        }),
        &mut state,
    );
    assert!(reindex_res.is_ok());
    let text = reindex_res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(text.contains("Project Re-Indexed Successfully"));

    // 2. Search hits symbol FTS
    let search_res = handle_tool_call(
        "project_context",
        json!({
            "operation": "search",
            "project_path": temp_dir.to_str().unwrap(),
            "query": "PaymentGateway"
        }),
        &mut state,
    );
    assert!(search_res.is_ok());
    let text = search_res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(text.contains("PaymentGateway"));

    // 3. Search hits content FTS
    let content_res = handle_tool_call(
        "project_context",
        json!({
            "operation": "search",
            "project_path": temp_dir.to_str().unwrap(),
            "query": "timeout"
        }),
        &mut state,
    );
    assert!(content_res.is_ok());
    let text = content_res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(text.contains("payment.rs"));

    // 4. Learn Alias
    let learn_res = handle_tool_call(
        "project_context",
        json!({
            "operation": "learn_alias",
            "project_path": temp_dir.to_str().unwrap(),
            "alias_term": "thanh toán thẻ",
            "relative_path": "src/payment.rs",
            "resolved_symbol": "PaymentGateway"
        }),
        &mut state,
    );
    assert!(learn_res.is_ok());
    let text = learn_res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(text.contains("Alias Learned Successfully"));

    // 5. Search hits Alias Cache
    let alias_search_res = handle_tool_call(
        "project_context",
        json!({
            "operation": "search",
            "project_path": temp_dir.to_str().unwrap(),
            "query": "thanh toán thẻ"
        }),
        &mut state,
    );
    assert!(alias_search_res.is_ok());
    let text = alias_search_res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(text.contains("alias cache"));
    assert!(text.contains("src/payment.rs"));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_project_context_skeleton_mode() {
    let temp_dir = std::env::temp_dir().join(format!("ag_skel_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(temp_dir.join("src"));
    let test_file = temp_dir.join("src/large.rs");

    let mut lines = Vec::new();
    lines.push("pub struct BigService;".to_string());
    lines.push("impl BigService {".to_string());
    lines.push("    pub fn heavy_computation(&self) {".to_string());
    for i in 0..320 {
        lines.push(format!("        let var_{} = {};", i, i));
    }
    lines.push("    }".to_string());
    lines.push("}".to_string());

    std::fs::write(&test_file, lines.join("\n")).unwrap();

    let mut state = ServerState::new();

    // Reading file > 300 LOC automatically triggers skeleton mode
    let read_res = handle_tool_call(
        "project_context",
        json!({
            "operation": "read",
            "project_path": temp_dir.to_str().unwrap(),
            "relative_path": "src/large.rs"
        }),
        &mut state,
    );
    assert!(read_res.is_ok());
    let text = read_res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(text.contains("AST Structural Skeleton"));
    assert!(text.contains("pub fn heavy_computation(&self)"));
    assert!(text.contains("Token Saver Mode"));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_navigate_scopes_validation_and_file_like_escaping() {
    let temp_dir = std::env::temp_dir().join(format!("ag_nav_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(temp_dir.join("src")).unwrap();
    std::fs::write(temp_dir.join("src/my_file.rs"), "pub fn exact_underscore() {}\n").unwrap();
    std::fs::write(temp_dir.join("src/myXfile.rs"), "pub fn wildcard_trap() {}\n").unwrap();

    let mut state = ServerState::new();
    let _ = handle_tool_call(
        "project_context",
        json!({"operation": "reindex", "project_path": temp_dir.to_str().unwrap()}),
        &mut state,
    )
    .unwrap();

    // Unknown scope -> Err((-32602, ...))
    let bad_scope = handle_tool_call(
        "project_context",
        json!({
            "operation": "navigate",
            "project_path": temp_dir.to_str().unwrap(),
            "query": "my_file",
            "scope": "invalid_scope"
        }),
        &mut state,
    );
    assert!(matches!(bad_scope, Err((-32602, _))));

    // Whitespace-only query -> Err((-32602, ...))
    let ws_query = handle_tool_call(
        "project_context",
        json!({
            "operation": "navigate",
            "project_path": temp_dir.to_str().unwrap(),
            "query": "   \t ",
            "scope": "files"
        }),
        &mut state,
    );
    assert!(matches!(ws_query, Err((-32602, _))));

    // Unknown operation -> Err((-32602, ...))
    let bad_op = handle_tool_call(
        "project_context",
        json!({
            "operation": "nonexistent_op",
            "project_path": temp_dir.to_str().unwrap()
        }),
        &mut state,
    );
    assert!(matches!(bad_op, Err((-32602, _))));

    // scope="files" searches files with escaped LIKE '_' wildcard
    let files_nav = handle_tool_call(
        "project_context",
        json!({
            "operation": "navigate",
            "project_path": temp_dir.to_str().unwrap(),
            "query": "my_file",
            "scope": "files"
        }),
        &mut state,
    )
    .unwrap();
    let files_text = files_nav["content"][0]["text"].as_str().unwrap();
    assert!(files_text.contains("src/my_file.rs"), "Got: {}", files_text);
    assert!(!files_text.contains("myXfile.rs"), "LIKE '_' must be escaped: {}", files_text);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_symbols_ast_modifiers_and_dataflow_escaping() {
    let temp_dir = std::env::temp_dir().join(format!("ag_sym_df_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(temp_dir.join("src")).unwrap();
    std::fs::write(
        temp_dir.join("src/mod_fns.rs"),
        "pub(crate) fn internal_helper() {}\npub async fn fetch_async() {}\nasync fn bg_task() {}\n",
    )
    .unwrap();
    std::fs::write(
        temp_dir.join("src/client.ts"),
        "export async function loadUser(id: string) { return id; }\n",
    )
    .unwrap();

    let mut state = ServerState::new();
    let rs_syms = handle_tool_call(
        "project_context",
        json!({
            "operation": "symbols",
            "project_path": temp_dir.to_str().unwrap(),
            "relative_path": "src/mod_fns.rs"
        }),
        &mut state,
    )
    .unwrap();
    let rs_text = rs_syms["content"][0]["text"].as_str().unwrap();
    assert!(rs_text.contains("internal_helper"), "Got: {}", rs_text);
    assert!(rs_text.contains("fetch_async"), "Got: {}", rs_text);
    assert!(rs_text.contains("bg_task"), "Got: {}", rs_text);

    let ts_syms = handle_tool_call(
        "project_context",
        json!({
            "operation": "symbols",
            "project_path": temp_dir.to_str().unwrap(),
            "relative_path": "src/client.ts"
        }),
        &mut state,
    )
    .unwrap();
    let ts_text = ts_syms["content"][0]["text"].as_str().unwrap();
    assert!(ts_text.contains("loadUser"), "Got: {}", ts_text);

    // Dataflow LIKE escaping & backward wording ("to" instead of "from")
    {
        let db = crate::context::db::CodeGraphDb::open_for_project(&temp_dir).unwrap();
        db.upsert_file("src/a.rs", "ha", 10, 1).unwrap();
        db.upsert_file("src/b.rs", "hb", 10, 1).unwrap();
        db.insert_symbol("src/a.rs::caller", "caller", "function", "src/a.rs", None, 1, 5, None).unwrap();
        db.insert_symbol("src/b.rs::user_sink", "user_sink", "function", "src/b.rs", None, 1, 5, None).unwrap();
        db.insert_symbol("src/b.rs::userXsink", "userXsink", "function", "src/b.rs", None, 6, 10, None).unwrap();
        db.conn
            .execute(
                "INSERT INTO data_flow_edges (caller_symbol_id, callee_symbol_id, source_variable, target_parameter, arg_index, call_line, flow_type, confidence)
                 VALUES ('src/a.rs::caller', 'src/b.rs::user_sink', 'raw_id', 'user_id', 0, 12, 'direct', 0.9),
                        ('src/a.rs::caller', 'src/b.rs::userXsink', 'raw_id', 'userXid', 0, 14, 'direct', 0.9)",
                [],
            )
            .unwrap();
    }
    let df_res = handle_tool_call(
        "project_context",
        json!({
            "operation": "data_flow",
            "project_path": temp_dir.to_str().unwrap(),
            "target_symbol": "user_sink"
        }),
        &mut state,
    )
    .unwrap();
    let df_text = df_res["content"][0]["text"].as_str().unwrap();
    assert!(df_text.contains("passed `raw_id` to `src/b.rs::user_sink`"), "Got: {}", df_text);
    assert!(!df_text.contains("userXsink"), "LIKE '_' must be escaped in data_flow: {}", df_text);

    // enrich_graph validates confidence & semantic_query rejects empty query
    let bad_enrich = handle_tool_call(
        "project_context",
        json!({
            "operation": "enrich_graph",
            "project_path": temp_dir.to_str().unwrap(),
            "edges": [{"source": "a", "target": "b", "confidence": 1.5}]
        }),
        &mut state,
    )
    .unwrap();
    assert!(bad_enrich["content"][0]["text"].as_str().unwrap().contains("Invalid confidence"));

    let bad_sem = handle_tool_call(
        "project_context",
        json!({
            "operation": "semantic_query",
            "project_path": temp_dir.to_str().unwrap(),
            "query": "   "
        }),
        &mut state,
    )
    .unwrap();
    assert!(bad_sem["content"][0]["text"].as_str().unwrap().contains("Error:"));

    let _ = std::fs::remove_dir_all(&temp_dir);
}
