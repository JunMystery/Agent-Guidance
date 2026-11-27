use serde_json::Value;
use std::path::Path;

use crate::context::graph_rag::{CommunityLevel, GraphRagEngine, GraphRagQueryMode};
use crate::mcp::state::ServerState;
use super::helpers::detect_project_architecture;

pub(crate) fn handle_architecture(proj_path: &Path, state: &mut ServerState) -> String {
    let arch_pattern = detect_project_architecture(proj_path);
    state.active_architecture_pattern = Some(arch_pattern.clone());
    let _ = ServerState::save_persisted_architecture(proj_path, &arch_pattern);

    let engine = GraphRagEngine::new(proj_path);
    let hierarchy = engine.load_or_build(&arch_pattern);

    let subsystems = if hierarchy.communities.is_empty() {
        "No communities indexed yet. Run `project_context(operation=\"reindex\")` to build hierarchy.".to_string()
    } else {
        hierarchy
            .get_by_level(CommunityLevel::MacroSubsystem)
            .iter()
            .map(|c| {
                format!(
                    "- **{}** (Layer: {}): {}",
                    c.summary.title, c.summary.layer, c.summary.description
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let mermaid_dag = engine.architecture_mermaid(&arch_pattern);

    let mut healing_report = String::new();
    if let Ok(db) = crate::context::db::CodeGraphDb::open_for_project(proj_path) {
        if let Ok(cycles) = crate::context::healing::detect_file_cycles(&db.conn) {
            if !cycles.is_empty() {
                healing_report.push_str(&format!("\n\n### Architecture Healing Sentinel: {} Circular Dependencies Detected\n", cycles.len()));
                for (i, c) in cycles.iter().take(5).enumerate() {
                    healing_report.push_str(&format!("{}. {}\n", i + 1, c.join(" -> ")));
                }
            }
        }
        if let Ok(orphans) = crate::context::healing::detect_orphan_symbols(&db.conn, 10) {
            if !orphans.is_empty() {
                healing_report.push_str(&format!("\n\n### Architecture Healing Sentinel: Candidate Orphan Symbols\n"));
                for o in orphans.iter().take(5) {
                    healing_report.push_str(&format!("- `{}` in `{}:L{}` ({})\n", o.name, o.file_path, o.line, o.kind));
                }
            }
        }
    }

    format!(
        "# Project Architecture Analysis (GraphRAG)\n\n- Detected / Memorized Pattern: **{}**\n- Workspace Root: {}\n- Total Hierarchical Communities: {}\n- Persistence: Memorized in `.agent-context/architecture.json`\n\n### Core Community Subsystems:\n{}\n\n### Architecture Dependency DAG:\n```mermaid\n{}\n```{red}",
        arch_pattern,
        proj_path.display(),
        hierarchy.communities.len(),
        subsystems,
        mermaid_dag,
        red = healing_report
    )
}

pub(crate) fn handle_graph_rag(
    arguments: &Value,
    query: &str,
    proj_path: &Path,
) -> String {
    let mode_str = arguments
        .get("mode")
        .and_then(|m| m.as_str())
        .unwrap_or("drift");
    let mode = GraphRagQueryMode::from_str(mode_str);
    let arch_pattern = detect_project_architecture(proj_path);
    let engine = GraphRagEngine::new(proj_path);
    let blast_radius = if let Ok(dag) = engine.symbol_blast_radius_mermaid(query.trim()) {
        if dag.lines().count() > 2 {
            format!("\n\n### Dependency Flow & Blast Radius (Mermaid DAG):\n```mermaid\n{}\n```", dag)
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    match engine.query(query, mode, &arch_pattern) {
        Ok(res) => {
            format!(
                "# {}\n\nMode: {:?}\n\n{}{}",
                res.title,
                res.mode,
                res.sections.join("\n\n"),
                blast_radius
            )
        }
        Err(e) => format!("GraphRAG query error: {}", e),
    }
}

pub(crate) fn handle_reusable(proj_path: &Path) -> String {
    let engine = GraphRagEngine::new(proj_path);
    match engine.analyze_reusability() {
        Ok(report) => report,
        Err(e) => format!("Reusability analysis error: {}", e),
    }
}

fn escape_like_literal(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\\' | '%' | '_' => {
                escaped.push('\\');
                escaped.push(ch);
            }
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn parse_symbol_and_path<'a>(query: &'a str, rel_path: Option<&'a str>) -> (&'a str, String) {
    let trimmed = query.trim();
    let (inferred_path, clean_sym) = if let Some((file, sym)) = trimmed.split_once('#') {
        (Some(file.trim()), sym.trim())
    } else {
        (None, trimmed)
    };
    let raw_file = rel_path.or(inferred_path).unwrap_or("").trim().replace('\\', "/");
    let clean_file = raw_file.trim_start_matches("./").trim_start_matches('/').to_string();
    (clean_sym, clean_file)
}

pub(crate) fn handle_callers(proj_path: &Path, query: &str, rel_path: Option<&str>) -> String {
    let (clean_sym, file_filter) = parse_symbol_and_path(query, rel_path);
    if clean_sym.is_empty() {
        return "Error: query symbol is required for callers operation.".to_string();
    }
    let db = match crate::context::db::CodeGraphDb::open_for_project(proj_path) {
        Ok(db) => db,
        Err(e) => return format!("Failed to open code graph: {}", e),
    };

    let escaped_filter = escape_like_literal(&file_filter);
    let mut stmt = match db.conn.prepare(
        "SELECT s.name, s.file_path, s.start_line, e.edge_type, e.weight
         FROM symbol_edges e
         JOIN symbols s ON e.source_id = s.id
         WHERE e.target_id IN (
             SELECT id FROM symbols
             WHERE (name = ?1 OR id = ?1)
               AND (?2 = '' OR REPLACE(file_path, '\\', '/') = ?2 OR REPLACE(file_path, '\\', '/') LIKE '%' || ?3 ESCAPE '\\')
         )
         ORDER BY e.weight DESC
         LIMIT 50"
    ) {
        Ok(s) => s,
        Err(e) => return format!("SQL error: {}", e),
    };

    let mut callers: Vec<String> = stmt
        .query_map(rusqlite::params![clean_sym, file_filter, escaped_filter], |row| {
            let name: String = row.get(0)?;
            let file: String = row.get(1)?;
            let line: usize = row.get(2)?;
            let etype: String = row.get(3)?;
            let weight: f64 = row.get(4)?;
            Ok(format!("- `{}` in `{}:L{}` [{}, weight: {:.1}]", name, file, line, etype, weight))
        })
        .map(|iter| iter.filter_map(|r| r.ok()).collect())
        .unwrap_or_default();

    // Milestone v1.9.0: Multi-Workspace Federated Callers
    if file_filter.is_empty() {
        let fed = crate::context::federation::federated_search_callers(proj_path, clean_sym, 20);
        for f in fed {
            if f.repo != "local" {
                callers.push(format!("- `{}` in `[repo:{}] {}:L{}` [{}]", f.symbol_name, f.repo, f.file_path, f.line, f.edge_type));
            }
        }
    }

    if callers.is_empty() {
        format!("# Callers of '{}'\n\nNo incoming callers found in symbol graph.", clean_sym)
    } else {
        format!("# Callers of '{}' (Found: {})\n\n{}", clean_sym, callers.len(), callers.join("\n"))
    }
}

pub(crate) fn handle_callees(proj_path: &Path, query: &str, rel_path: Option<&str>) -> String {
    let (clean_sym, file_filter) = parse_symbol_and_path(query, rel_path);
    if clean_sym.is_empty() {
        return "Error: query symbol is required for callees operation.".to_string();
    }
    let db = match crate::context::db::CodeGraphDb::open_for_project(proj_path) {
        Ok(db) => db,
        Err(e) => return format!("Failed to open code graph: {}", e),
    };

    let escaped_filter = escape_like_literal(&file_filter);
    let mut stmt = match db.conn.prepare(
        "SELECT s.name, s.file_path, s.start_line, e.edge_type, e.weight
         FROM symbol_edges e
         JOIN symbols s ON e.target_id = s.id
         WHERE e.source_id IN (
             SELECT id FROM symbols
             WHERE (name = ?1 OR id = ?1)
               AND (?2 = '' OR REPLACE(file_path, '\\', '/') = ?2 OR REPLACE(file_path, '\\', '/') LIKE '%' || ?3 ESCAPE '\\')
         )
         ORDER BY e.weight DESC
         LIMIT 50"
    ) {
        Ok(s) => s,
        Err(e) => return format!("SQL error: {}", e),
    };

    let mut callees: Vec<String> = stmt
        .query_map(rusqlite::params![clean_sym, file_filter, escaped_filter], |row| {
            let name: String = row.get(0)?;
            let file: String = row.get(1)?;
            let line: usize = row.get(2)?;
            let etype: String = row.get(3)?;
            let weight: f64 = row.get(4)?;
            Ok(format!("- `{}` in `{}:L{}` [{}, weight: {:.1}]", name, file, line, etype, weight))
        })
        .map(|iter| iter.filter_map(|r| r.ok()).collect())
        .unwrap_or_default();

    // Milestone v1.9.0: Multi-Workspace Federated Callees
    if file_filter.is_empty() {
        let fed = crate::context::federation::federated_search_callees(proj_path, clean_sym, 20);
        for f in fed {
            if f.repo != "local" {
                callees.push(format!("- `{}` in `[repo:{}] {}:L{}` [{}]", f.symbol_name, f.repo, f.file_path, f.line, f.edge_type));
            }
        }
    }

    if callees.is_empty() {
        format!("# Callees of '{}'\n\nNo outgoing callees found in symbol graph.", clean_sym)
    } else {
        format!("# Callees of '{}' (Found: {})\n\n{}", clean_sym, callees.len(), callees.join("\n"))
    }
}

pub(crate) fn handle_blast_radius(proj_path: &Path, query: &str, rel_path: Option<&str>) -> String {
    let (clean_sym, file_filter) = parse_symbol_and_path(query, rel_path);
    if clean_sym.is_empty() {
        return "Error: query symbol is required for blast_radius operation.".to_string();
    }

    let db = match crate::context::db::CodeGraphDb::open_for_project(proj_path) {
        Ok(db) => db,
        Err(e) => return format!("Failed to open code graph: {}", e),
    };

    let escaped_filter = escape_like_literal(&file_filter);
    let matched_count: i64 = db.conn.query_row(
        "SELECT COUNT(*) FROM symbols
         WHERE (name = ?1 OR id = ?1)
           AND (?2 = '' OR REPLACE(file_path, '\\', '/') = ?2 OR REPLACE(file_path, '\\', '/') LIKE '%' || ?3 ESCAPE '\\')",
        rusqlite::params![clean_sym, file_filter, escaped_filter],
        |row| row.get(0),
    ).unwrap_or(0);

    if matched_count == 0 {
        let scope_note = if file_filter.is_empty() {
            String::new()
        } else {
            format!(" in '{}'", file_filter)
        };
        return format!(
            "# Blast Radius Impact Analysis for '{}'\n\nSymbol '{}' not found in code graph index{}. Run `project_context(operation=\"search\", query=\"{}\")` or `project_context(operation=\"reindex\")`.",
            clean_sym, clean_sym, scope_note, clean_sym
        );
    }

    let upstream_count: usize = db.conn.query_row(
        "SELECT COUNT(DISTINCT e.source_id) FROM symbol_edges e
         WHERE e.target_id IN (
             SELECT id FROM symbols
             WHERE (name = ?1 OR id = ?1)
               AND (?2 = '' OR REPLACE(file_path, '\\', '/') = ?2 OR REPLACE(file_path, '\\', '/') LIKE '%' || ?3 ESCAPE '\\')
         )",
        rusqlite::params![clean_sym, file_filter, escaped_filter],
        |row| row.get::<_, i64>(0),
    ).unwrap_or(0).max(0) as usize;

    let downstream_count: usize = db.conn.query_row(
        "SELECT COUNT(DISTINCT e.target_id) FROM symbol_edges e
         WHERE e.source_id IN (
             SELECT id FROM symbols
             WHERE (name = ?1 OR id = ?1)
               AND (?2 = '' OR REPLACE(file_path, '\\', '/') = ?2 OR REPLACE(file_path, '\\', '/') LIKE '%' || ?3 ESCAPE '\\')
         )",
        rusqlite::params![clean_sym, file_filter, escaped_filter],
        |row| row.get::<_, i64>(0),
    ).unwrap_or(0).max(0) as usize;

    let mut caller_names = Vec::new();
    if let Ok(mut in_stmt) = db.conn.prepare(
        "SELECT DISTINCT s.name FROM symbol_edges e
         JOIN symbols s ON e.source_id = s.id
         WHERE e.target_id IN (
             SELECT id FROM symbols
             WHERE (name = ?1 OR id = ?1)
               AND (?2 = '' OR REPLACE(file_path, '\\', '/') = ?2 OR REPLACE(file_path, '\\', '/') LIKE '%' || ?3 ESCAPE '\\')
         )
         LIMIT 10"
    ) {
        if let Ok(rows) = in_stmt.query_map(rusqlite::params![clean_sym, file_filter, escaped_filter], |r| r.get::<_, String>(0)) {
            caller_names = rows.flatten().collect();
        }
    }

    let mut callee_names = Vec::new();
    if let Ok(mut out_stmt) = db.conn.prepare(
        "SELECT DISTINCT s.name FROM symbol_edges e
         JOIN symbols s ON e.target_id = s.id
         WHERE e.source_id IN (
             SELECT id FROM symbols
             WHERE (name = ?1 OR id = ?1)
               AND (?2 = '' OR REPLACE(file_path, '\\', '/') = ?2 OR REPLACE(file_path, '\\', '/') LIKE '%' || ?3 ESCAPE '\\')
         )
         LIMIT 10"
    ) {
        if let Ok(rows) = out_stmt.query_map(rusqlite::params![clean_sym, file_filter, escaped_filter], |r| r.get::<_, String>(0)) {
            callee_names = rows.flatten().collect();
        }
    }

    let dag = crate::context::graph_rag::mermaid::generate_blast_radius_mermaid(
        clean_sym,
        &caller_names,
        &callee_names,
    );
    let (risk_score, risk_level) =
        crate::context::graph_rag::subgraph_bundle::compute_blast_radius(upstream_count, downstream_count);

    format!(
        "# Blast Radius Impact Analysis for '{}'\n\n- **Risk Level**: {} (Score: {:.1}/10)\n- **Direct Callers (Upstream Impact)**: {}\n- **Dependencies (Downstream Impact)**: {}\n\n### Dependency DAG (Mermaid):\n```mermaid\n{}\n```",
        clean_sym,
        risk_level,
        risk_score,
        upstream_count,
        downstream_count,
        if dag.trim().is_empty() { "graph TD\n    None" } else { dag.trim() }
    )
}
