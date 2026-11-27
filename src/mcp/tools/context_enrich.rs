use crate::context::db::CodeGraphDb;
use serde_json::Value;
use std::path::Path;

pub(crate) fn handle_enrich_graph(arguments: &Value, proj_path: &Path) -> String {
    let db = match CodeGraphDb::open_for_project(proj_path) {
        Ok(d) => d,
        Err(e) => return format!("Error: Failed to open CodeGraphDb: {}", e),
    };

    let agent_id = arguments
        .get("agent_id")
        .and_then(|a| a.as_str())
        .unwrap_or("agent");

    let mut edges_added = 0usize;
    let mut summaries_added = 0usize;
    let mut errors = Vec::new();

    let record_edge = |src: &str, tgt: &str, rel: &str, desc: Option<&str>, conf: f64| -> Result<(), String> {
        if src.trim().is_empty() || tgt.trim().is_empty() {
            return Err("Semantic edge requires non-empty 'source' and 'target'.".to_string());
        }
        if !conf.is_finite() || !(0.0..=1.0).contains(&conf) {
            return Err(format!(
                "Invalid confidence {} for edge '{} -> {}' (must be within 0.0..=1.0).",
                conf, src, tgt
            ));
        }
        db.upsert_semantic_edge(src.trim(), tgt.trim(), rel.trim(), desc, conf, agent_id)
            .map_err(|e| format!("Failed to upsert semantic edge '{} -> {}': {}", src, tgt, e))
    };

    // 1. Batch edges
    if let Some(arr) = arguments.get("edges").and_then(|v| v.as_array()) {
        for item in arr {
            let src = item.get("source").or_else(|| item.get("source_symbol")).and_then(|s| s.as_str());
            let tgt = item.get("target").or_else(|| item.get("target_symbol")).and_then(|s| s.as_str());
            let rel = item.get("relation").or_else(|| item.get("relation_type")).and_then(|s| s.as_str()).unwrap_or("business_flow");
            let desc = item.get("description").and_then(|s| s.as_str());
            let conf = item.get("confidence").and_then(|c| c.as_f64()).unwrap_or(0.9);

            match (src, tgt) {
                (Some(s), Some(t)) => match record_edge(s, t, rel, desc, conf) {
                    Ok(()) => edges_added += 1,
                    Err(e) => errors.push(e),
                },
                _ => errors.push("Semantic edge entry missing 'source' or 'target'.".to_string()),
            }
        }
    } else if let (Some(s), Some(t)) = (
        arguments.get("source").or_else(|| arguments.get("source_symbol")).and_then(|s| s.as_str()),
        arguments.get("target").or_else(|| arguments.get("target_symbol")).and_then(|s| s.as_str()),
    ) {
        let rel = arguments.get("relation").or_else(|| arguments.get("relation_type")).and_then(|s| s.as_str()).unwrap_or("business_flow");
        let desc = arguments.get("description").and_then(|s| s.as_str());
        let conf = arguments.get("confidence").and_then(|c| c.as_f64()).unwrap_or(0.9);
        match record_edge(s, t, rel, desc, conf) {
            Ok(()) => edges_added += 1,
            Err(e) => errors.push(e),
        }
    }

    let record_summary = |m: &str, ti: &str, su: &str, tags: Option<&str>| -> Result<(), String> {
        if m.trim().is_empty() || ti.trim().is_empty() || su.trim().is_empty() {
            return Err("Domain summary requires non-empty 'module_path', 'title', and 'summary'.".to_string());
        }
        db.upsert_domain_summary(m.trim(), ti.trim(), su.trim(), tags, agent_id)
            .map_err(|e| format!("Failed to upsert domain summary '{}': {}", m, e))
    };

    // 2. Batch summaries
    if let Some(arr) = arguments.get("summaries").and_then(|v| v.as_array()) {
        for item in arr {
            let mod_path = item.get("module_path").and_then(|s| s.as_str());
            let title = item.get("title").and_then(|s| s.as_str());
            let summary = item.get("summary").and_then(|s| s.as_str());
            let tags = item.get("tags").and_then(|s| s.as_str());

            match (mod_path, title, summary) {
                (Some(m), Some(ti), Some(su)) => match record_summary(m, ti, su, tags) {
                    Ok(()) => summaries_added += 1,
                    Err(e) => errors.push(e),
                },
                _ => errors.push("Domain summary entry missing 'module_path', 'title', or 'summary'.".to_string()),
            }
        }
    } else if let (Some(m), Some(ti), Some(su)) = (
        arguments.get("module_path").and_then(|s| s.as_str()),
        arguments.get("title").and_then(|s| s.as_str()),
        arguments.get("summary").and_then(|s| s.as_str()),
    ) {
        let tags = arguments.get("tags").and_then(|s| s.as_str());
        match record_summary(m, ti, su, tags) {
            Ok(()) => summaries_added += 1,
            Err(e) => errors.push(e),
        }
    }

    if edges_added == 0 && summaries_added == 0 && !errors.is_empty() {
        return format!(
            "# GraphRAG Semantic Enrichment [FAILED]\n\nErrors:\n- {}",
            errors.join("\n- ")
        );
    }

    let err_section = if errors.is_empty() {
        String::new()
    } else {
        format!("\n\n### Warnings / Errors:\n- {}", errors.join("\n- "))
    };

    format!(
        "# GraphRAG Semantic Enrichment [OK]\n\n- Semantic Edges Added/Updated: {}\n- Domain Summaries Added/Updated: {}\n- Enriched By: `{}`\n- Persistence: Saved in `.agent-context/code_graph.db`{}",
        edges_added, summaries_added, agent_id, err_section
    )
}

pub(crate) fn handle_query_semantic(arguments: &Value, proj_path: &Path) -> String {
    let query = arguments
        .get("query")
        .or_else(|| arguments.get("symbol"))
        .and_then(|q| q.as_str())
        .unwrap_or("")
        .trim();

    if query.is_empty() {
        return "Error: 'query' (or 'symbol') parameter is required for semantic_query operation.".to_string();
    }

    let db = match CodeGraphDb::open_for_project(proj_path) {
        Ok(d) => d,
        Err(e) => return format!("Error: Failed to open CodeGraphDb: {}", e),
    };

    let mut out = format!("# Semantic Knowledge for '{}'\n\n", query);
    let mut query_errors = Vec::new();

    // 1. Domain summaries
    match db.search_domain_summaries_fts(query, 5) {
        Ok(summaries) if !summaries.is_empty() => {
            out.push_str("### Domain Summaries:\n");
            for s in summaries {
                out.push_str(&format!("- **{}** (`{}`): {}\n", s.title, s.module_path, s.summary));
            }
            out.push('\n');
        }
        Ok(_) => {}
        Err(e) => query_errors.push(format!("Domain summary query error: {}", e)),
    }

    // 2. Semantic edges
    match db.query_semantic_edges_for_symbol(query) {
        Ok(edges) if !edges.is_empty() => {
            out.push_str("### Semantic Edges:\n");
            for e in edges {
                let desc = e.description.as_deref().unwrap_or("—");
                out.push_str(&format!(
                    "- `{}` --[{}]--> `{}` (conf: {:.2}): {}\n",
                    e.source_symbol, e.relation_type, e.target_symbol, e.confidence, desc
                ));
            }
            out.push('\n');
        }
        Ok(_) => {}
        Err(e) => query_errors.push(format!("Semantic edge query error: {}", e)),
    }

    if !query_errors.is_empty() {
        out.push_str(&format!("### Errors:\n- {}\n", query_errors.join("\n- ")));
    } else if !out.contains("###") {
        out.push_str("No semantic knowledge or domain summaries found matching query.");
    }

    out
}
