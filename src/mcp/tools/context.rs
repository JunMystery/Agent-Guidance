use serde_json::Value;

use crate::context::graph_rag::ensure_fresh_graph;
use crate::context::indexer::IncrementalIndexer;
use crate::context::scanner::scan_project;
use crate::mcp::state::ServerState;
use super::context_graph::{
    handle_architecture, handle_blast_radius, handle_callees, handle_callers, handle_graph_rag,
    handle_reusable,
};
use super::context_lsp::{handle_definition, handle_type_definition};
use super::context_dataflow::handle_data_flow;
use super::context_read::handle_read;
use super::context_search::{handle_navigate, handle_search};
use super::context_bundle::handle_subgraph_bundle;
use super::context_enrich::{handle_enrich_graph, handle_query_semantic};
use super::context_symbols::{handle_learn_alias, handle_references, handle_symbols};
use super::helpers::{detect_project_path_with_targets, ensure_not_cancelled};

pub(crate) fn handle(
    arguments: Value,
    state: &mut ServerState,
) -> Result<String, (i32, String)> {
    ensure_not_cancelled(state)?;
    let op = arguments
        .get("operation")
        .and_then(|o| o.as_str())
        .unwrap_or("tree");
    let proj_path_arg = arguments
        .get("project_path")
        .and_then(|p| p.as_str())
        .unwrap_or(".");
    let target_paths = super::gate_edit::extract_target_paths(&arguments);
    let proj_path = detect_project_path_with_targets(proj_path_arg, &target_paths, state);
    let is_stateful_op = matches!(
        op,
        "architecture"
            | "reindex"
            | "learn_alias"
            | "enrich_graph"
            | "enrich"
            | "read_and_authorize"
            | "read_auth"
    ) || arguments
        .get("authorize")
        .or_else(|| arguments.get("pre_authorize"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if state.project_path.is_none() || is_stateful_op {
        state.update_project_path(&proj_path);
    }
    let query = arguments
        .get("query")
        .and_then(|q| q.as_str())
        .unwrap_or("");
    let intent = arguments
        .get("intent")
        .and_then(|i| i.as_str());
    let rel_path = arguments
        .get("relative_path")
        .and_then(|r| r.as_str())
        .unwrap_or("");

    let sync_notice = if op != "reindex" && op != "tree" {
        // 1. Write-through flush of dirty files queued during active session
        let dirty_paths = state.drain_dirty_files();
        if !dirty_paths.is_empty() {
            let _ = crate::context::indexer::invalidator::sync_dirty_files(&proj_path, &dirty_paths);
        }

        // 2. Targeted JIT sync for requested file(s) (bypasses 3s debounce, < 10ms)
        let mut synced = Vec::new();
        for raw_target in target_paths.iter().chain(std::iter::once(&rel_path.to_string())) {
            let spec = super::helpers::TargetPathSpec::parse(raw_target);
            if !spec.clean_path.is_empty()
                && !spec.clean_path.starts_with("linked:")
                && !synced.contains(&spec.clean_path)
            {
                let _ = crate::context::graph_rag::jit_sync::ensure_fresh_targeted_file(
                    &proj_path,
                    &spec.clean_path,
                );
                synced.push(spec.clean_path);
            }
        }

        match ensure_fresh_graph(&proj_path, 3) {
            Ok(Some(report)) if report.files_indexed > 0 => {
                format!(
                    "> [!NOTE]\n> **GraphRAG Auto-Sync**: Fresh code graph updated ({} files indexed, {} symbols, {} edges in {}ms).\n\n",
                    report.files_indexed, report.symbols_extracted, report.edges_created, report.duration_ms
                )
            }
            _ => String::new(),
        }
    } else {
        String::new()
    };

    let resp = match op {
        "tree" => {
            let depth = arguments
                .get("max_depth")
                .and_then(|v| v.as_u64())
                .unwrap_or(3)
                .clamp(1, 5) as usize;
            let files = scan_project(&proj_path, depth);
            let file_list: Vec<String> = files
                .into_iter()
                .map(|f| format!("- {} ({})", f.path, f.file_type))
                .collect();
            format!(
                "# Project Tree (Depth Capped at {})\n\n{}",
                depth,
                file_list.join("\n")
            )
        }
        "read" | "cluster_read" | "read_cluster" | "read_and_authorize" | "read_auth" => {
            handle_read(&arguments, &proj_path, rel_path, state)
        }
        "search" => handle_search(query, &proj_path, intent, state),
        "navigate" => handle_navigate(&arguments, query, &proj_path, state)?,
        "learn_alias" => handle_learn_alias(&arguments, &proj_path, rel_path),
        "enrich_graph" | "enrich" => handle_enrich_graph(&arguments, &proj_path),
        "semantic_query" | "semantic" => handle_query_semantic(&arguments, &proj_path),
        "reindex" => {
            match IncrementalIndexer::new(&proj_path) {
                Ok(mut indexer) => match indexer.full_index() {
                    Ok(report) => {
                        state.ast_cache.clear();
                        let path = proj_path.clone();
                        let arch = state
                            .active_architecture_pattern
                            .clone()
                            .unwrap_or_else(|| super::helpers::detect_project_architecture(&proj_path));
                        std::thread::spawn(move || {
                            if let Ok(idx) = IncrementalIndexer::new(&path) {
                                let _ = idx.embed_symbols();
                                let _ = idx.embed_chunks();
                            }
                            let engine = crate::context::graph_rag::GraphRagEngine::new(&path);
                            let _ = engine.build_or_update(&arch);
                        });
                        format!(
                            "# Project Re-Indexed Successfully\n\n- Files Scanned: {}\n- Files Indexed: {}\n- Files Skipped: {}\n- Symbols Extracted: {}\n- Edges Created: {}\n- Content Chunks: {}\n- Duration: {}ms\n- Background Embedding & GraphRAG Communities: Rayon ML pool + Leiden refresh active\n- Persistence: `.agent-context/code_graph.db`",
                            report.files_scanned,
                            report.files_indexed,
                            report.files_skipped,
                            report.symbols_extracted,
                            report.edges_created,
                            report.chunks_created,
                            report.duration_ms
                        )
                    }
                    Err(e) => format!("Failed to reindex project: {}", e),
                },
                Err(e) => format!("Failed to reindex project: {}", e),
            }
        }
        "architecture" => handle_architecture(&proj_path, state),
        "graph_rag" => handle_graph_rag(&arguments, query, &proj_path),
        "reusable" | "shared" | "reusable_candidates" | "detect_duplicates" => {
            handle_reusable(&proj_path)
        }
        "symbols" | "structure" => handle_symbols(&proj_path, rel_path),
        "references" => handle_references(&arguments, &proj_path, query, rel_path),
        "callers" | "incoming_calls" => {
            let rel_opt = (!rel_path.is_empty()).then_some(rel_path);
            handle_callers(&proj_path, query, rel_opt)
        }
        "callees" | "outgoing_calls" => {
            let rel_opt = (!rel_path.is_empty()).then_some(rel_path);
            handle_callees(&proj_path, query, rel_opt)
        }
        "blast_radius" | "impact" => {
            let rel_opt = (!rel_path.is_empty()).then_some(rel_path);
            handle_blast_radius(&proj_path, query, rel_opt)
        }
        "definition" | "goto_definition" => handle_definition(&arguments, &proj_path, query, rel_path),
        "type_definition" => handle_type_definition(&arguments, &proj_path, query, rel_path),
        "subgraph_bundle" | "bundle" => {
            let target = arguments
                .get("target_symbol")
                .or_else(|| arguments.get("symbol"))
                .or_else(|| arguments.get("name"))
                .and_then(|t| t.as_str())
                .or_else(|| if !query.is_empty() { Some(query) } else { None })
                .unwrap_or("");
            handle_subgraph_bundle(&arguments, &proj_path, target, state)
        }
        "data_flow" | "dataflow" | "taint" | "flow" => {
            let target = if !query.trim().is_empty() {
                query
            } else {
                arguments
                    .get("target_symbol")
                    .or_else(|| arguments.get("symbol"))
                    .or_else(|| arguments.get("name"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
            };
            handle_data_flow(&proj_path, target)
        }
        _ => return Err((-32602, format!("Unknown project_context operation '{}'.", op))),
    };

    Ok(format!("{}{}", sync_notice, resp))
}
