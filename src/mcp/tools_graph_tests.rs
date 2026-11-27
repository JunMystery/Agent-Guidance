use serde_json::json;
use crate::context::db::CodeGraphDb;
use crate::mcp::state::ServerState;
use crate::mcp::tools::handle_tool_call;

#[test]
fn test_project_context_callers_and_callees() {
    let temp_dir = std::env::temp_dir().join(format!("ag_graph_mcp_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let _ = std::fs::create_dir_all(&temp_dir);

    {
        let db = CodeGraphDb::open_for_project(&temp_dir).unwrap();
        db.conn.execute(
            "INSERT INTO files (path, content_hash, size, modified_at, indexed_at) VALUES ('src/a.rs', 'hash1', 100, 100, 100)",
            [],
        ).unwrap();
        db.conn.execute(
            "INSERT INTO files (path, content_hash, size, modified_at, indexed_at) VALUES ('src/b.rs', 'hash2', 100, 100, 100)",
            [],
        ).unwrap();
        db.insert_symbol("src/a.rs::function::run::L10", "run", "function", "src/a.rs", None, 10, 20, None).unwrap();
        db.insert_symbol("src/b.rs::function::helper::L20", "helper", "function", "src/b.rs", None, 20, 30, None).unwrap();
        db.insert_edge("src/a.rs::function::run::L10", "src/b.rs::function::helper::L20", "calls", 1.0).unwrap();
    }

    let mut state = ServerState::new();
    state.plan_approved = true;
    state.set_stage("Build").unwrap();

    // 1. Test callers operation
    let res_callers = handle_tool_call(
        "project_context",
        json!({
            "operation": "callers",
            "query": "helper",
            "project_path": temp_dir.to_str().unwrap()
        }),
        &mut state,
    );
    assert!(res_callers.is_ok());
    let text_callers = res_callers.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(text_callers.contains("# Callers of 'helper'"));
    assert!(text_callers.contains("run"));
    assert!(text_callers.contains("src/a.rs:L10"));

    // 2. Test callees operation
    let res_callees = handle_tool_call(
        "project_context",
        json!({
            "operation": "callees",
            "query": "run",
            "project_path": temp_dir.to_str().unwrap()
        }),
        &mut state,
    );
    assert!(res_callees.is_ok());
    let text_callees = res_callees.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(text_callees.contains("# Callees of 'run'"));
    assert!(text_callees.contains("helper"));
    assert!(text_callees.contains("src/b.rs:L20"));

    // 3. Test blast radius operation
    let res_blast = handle_tool_call(
        "project_context",
        json!({
            "operation": "blast_radius",
            "query": "helper",
            "project_path": temp_dir.to_str().unwrap()
        }),
        &mut state,
    );
    assert!(res_blast.is_ok());
    let text_blast = res_blast.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(text_blast.contains("# Blast Radius Impact Analysis for 'helper'"));
    assert!(text_blast.contains("Direct Callers"));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_full_index_creates_symbol_edges() {
    let temp_dir = std::env::temp_dir().join(format!("ag_full_index_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let _ = std::fs::create_dir_all(temp_dir.join("src"));
    std::fs::write(
        temp_dir.join("src/lib.rs"),
        "pub fn foo() { bar(); }\npub fn bar() {}\n",
    ).unwrap();

    let mut indexer = crate::context::indexer::IncrementalIndexer::new(&temp_dir).unwrap();
    let report = indexer.full_index().unwrap();
    assert!(report.symbols_extracted > 0, "Symbols extracted must be > 0");
    assert!(report.edges_created > 0, "Edges created must be > 0, got {}", report.edges_created);

    let engine = crate::context::graph_rag::GraphRagEngine::new(&temp_dir);
    let hierarchy = engine.build_or_update("Clean_Architecture").unwrap();
    assert!(!hierarchy.communities.is_empty(), "Communities must be > 0, got {}", hierarchy.communities.len());

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_query_graph_data_on_actual_project() {
    let _ = crate::context::graph_rag::ensure_fresh_graph(std::path::Path::new("."), 0);
    let res = crate::dashboard::graph::query_graph_data(std::path::Path::new(".")).unwrap();
    if res["graph_available"].as_bool().unwrap_or(false) {
        let total_nodes = res["total_nodes"].as_u64().unwrap_or(0);
        let communities = res["communities"].as_array();
        if total_nodes > 0 && communities.is_some() {
            println!("Actual project graph: nodes={}, communities={}", total_nodes, communities.unwrap().len());
        }
    }
}

#[test]
fn test_neighborhood_and_drift_retrieval() {
    let temp_dir = std::env::temp_dir().join(format!("ag_neighborhood_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let _ = std::fs::create_dir_all(&temp_dir);

    {
        let db = CodeGraphDb::open_for_project(&temp_dir).unwrap();
        db.conn.execute(
            "INSERT INTO files (path, content_hash, size, modified_at, indexed_at) VALUES ('src/handler.rs', 'hash1', 100, 100, 100)",
            [],
        ).unwrap();
        db.insert_symbol("src/handler.rs::handle_req::L10", "handle_req", "function", "src/handler.rs", Some("pub fn handle_req()"), 10, 25, None).unwrap();
        db.insert_symbol("src/handler.rs::validate::L30", "validate", "function", "src/handler.rs", Some("fn validate()"), 30, 40, None).unwrap();
        db.insert_edge("src/handler.rs::handle_req::L10", "src/handler.rs::validate::L30", "calls", 1.0).unwrap();
        db.conn.execute(
            "INSERT INTO content_chunks (file_path, start_line, end_line, content_hash, chunk_text) VALUES ('src/handler.rs', 10, 25, 'hash1', 'pub fn handle_req() { validate(); }')",
            [],
        ).unwrap();
    }

    let db = CodeGraphDb::open_for_project(&temp_dir).unwrap();
    let hierarchy = crate::context::graph_rag::CommunityHierarchy::new("test", "Clean_Architecture");
    let nb_opt = crate::context::graph_rag::fetch_symbol_neighborhood(&db, &hierarchy, "handle_req").unwrap();
    assert!(nb_opt.is_some());
    let nb = nb_opt.unwrap();
    assert_eq!(nb.name, "handle_req");
    assert_eq!(nb.callees.len(), 1);
    assert_eq!(nb.callees[0].name, "validate");
    assert!(nb.code_excerpt.is_some());
    assert!(nb.code_excerpt.unwrap().contains("pub fn handle_req()"));
    assert!(nb.mermaid_dag.contains("handle_req"));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_jit_sync_execution() {
    let temp_dir = std::env::temp_dir().join(format!("jit_sync_exec_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let _ = std::fs::create_dir_all(temp_dir.join("src"));
    std::fs::write(temp_dir.join("src").join("lib.rs"), "pub fn test_val() -> i32 { 1 }").unwrap();

    let res = crate::context::graph_rag::ensure_fresh_graph(&temp_dir, 0);
    assert!(res.is_ok());

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_project_context_enrich_graph_and_query() {
    let temp_dir = std::env::temp_dir().join(format!("enrich_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let _ = std::fs::create_dir_all(&temp_dir);

    let mut state = ServerState::new();

    let enrich_res = handle_tool_call(
        "project_context",
        serde_json::json!({
            "operation": "enrich_graph",
            "project_path": temp_dir.to_str().unwrap(),
            "edges": [
                {
                    "source": "auth::login",
                    "target": "billing::process_invoice",
                    "relation": "triggers_workflow",
                    "description": "Auto creates invoice on business user login",
                    "confidence": 0.92
                }
            ],
            "summaries": [
                {
                    "module_path": "src/billing",
                    "title": "Billing Core",
                    "summary": "Handles invoice calculation and payment gateway dispatch",
                    "tags": "billing,invoice,payment"
                }
            ]
        }),
        &mut state,
    );
    assert!(enrich_res.is_ok(), "enrich_graph failed: {:?}", enrich_res);
    let text = enrich_res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(text.contains("Semantic Edges Added/Updated: 1"), "Result text: {}", text);
    assert!(text.contains("Domain Summaries Added/Updated: 1"), "Result text: {}", text);

    let query_res = handle_tool_call(
        "project_context",
        serde_json::json!({
            "operation": "semantic_query",
            "project_path": temp_dir.to_str().unwrap(),
            "query": "billing"
        }),
        &mut state,
    );
    assert!(query_res.is_ok(), "semantic_query failed: {:?}", query_res);
    let qtext = query_res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(qtext.contains("Billing Core"), "Query output: {}", qtext);
    assert!(qtext.contains("triggers_workflow"), "Query output: {}", qtext);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_blast_radius_disambiguates_relative_path_and_reports_missing_symbol() {
    let temp_dir = std::env::temp_dir().join(format!("ag_blast_disambig_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(temp_dir.join("src")).unwrap();
    std::fs::write(
        temp_dir.join("src/a.rs"),
        "pub fn process() {}\npub fn c1() { process(); }\npub fn c2() { process(); }\n",
    )
    .unwrap();
    std::fs::write(temp_dir.join("src/b.rs"), "pub fn process() {}\n").unwrap();

    let mut state = ServerState::new();
    let _ = handle_tool_call(
        "project_context",
        json!({
            "operation": "reindex",
            "project_path": temp_dir.to_str().unwrap()
        }),
        &mut state,
    )
    .unwrap();
    // 1. Missing symbol reports not-found instead of LOW (1.0/10)
    let missing = handle_tool_call(
        "project_context",
        json!({
            "operation": "blast_radius",
            "project_path": temp_dir.to_str().unwrap(),
            "query": "nonexistent_symbol_xyz"
        }),
        &mut state,
    )
    .unwrap();
    let m_text = missing["content"][0]["text"].as_str().unwrap();
    assert!(m_text.contains("not found"), "Got: {}", m_text);
    assert!(!m_text.contains("LOW"), "Missing symbol must not report LOW: {}", m_text);

    // 2. Disambiguate same-named symbol via relative_path
    let br_a = handle_tool_call(
        "project_context",
        json!({
            "operation": "blast_radius",
            "project_path": temp_dir.to_str().unwrap(),
            "query": "process",
            "relative_path": "src/a.rs"
        }),
        &mut state,
    )
    .unwrap();
    let a_text = br_a["content"][0]["text"].as_str().unwrap();
    assert!(a_text.contains("c1") && a_text.contains("c2"), "Expected c1 and c2 callers for src/a.rs, got: {}", a_text);
    assert!(a_text.contains("MEDIUM"), "Expected MEDIUM risk for src/a.rs, got: {}", a_text);

    let br_b = handle_tool_call(
        "project_context",
        json!({
            "operation": "blast_radius",
            "project_path": temp_dir.to_str().unwrap(),
            "query": "process",
            "relative_path": "src/b.rs"
        }),
        &mut state,
    )
    .unwrap();
    let b_text = br_b["content"][0]["text"].as_str().unwrap();
    assert!(!b_text.contains("c1") && !b_text.contains("c2"), "src/b.rs must not include src/a.rs callers: {}", b_text);
    assert!(b_text.contains("LOW"), "Got: {}", b_text);

    let _ = std::fs::remove_dir_all(&temp_dir);
}
