use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

use crate::context::ast::{AstEngine, AstLanguage};
use crate::context::db::CodeGraphDb;
use crate::context::scanner::scan_project;
use super::helpers::validate_path;

fn extract_symbols_via_db(proj_path: &Path, rel_path: &str) -> Vec<(usize, String)> {
    let db_path = proj_path.join(".agent-context").join("code_graph.db");
    if !db_path.exists() {
        return Vec::new();
    }
    let Ok(db) = CodeGraphDb::open_read_only(&db_path) else {
        return Vec::new();
    };
    let norm = rel_path.trim().trim_start_matches("./").replace('\\', "/");
    let mut stmt = match db.conn.prepare(
        "SELECT start_line, kind, name, signature
         FROM symbols
         WHERE REPLACE(file_path, '\\', '/') = ?1
         ORDER BY start_line ASC",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let Ok(rows) = stmt.query_map([norm], |row| {
        let line: i64 = row.get(0)?;
        let kind: String = row.get(1)?;
        let name: String = row.get(2)?;
        let sig: Option<String> = row.get(3)?;
        Ok((line.max(1) as usize, kind, name, sig))
    }) else {
        return Vec::new();
    };

    rows.flatten()
        .map(|(line, kind, name, sig)| {
            let label = sig
                .as_deref()
                .and_then(|s| s.lines().next())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| format!("[{}] {}", kind, s))
                .unwrap_or_else(|| format!("[{}] {}", kind, name));
            (line, label)
        })
        .collect()
}

fn is_modifier_symbol_line(trimmed: &str) -> bool {
    if trimmed.is_empty()
        || trimmed.starts_with("//")
        || trimmed.starts_with("/*")
        || trimmed.starts_with('*')
        || trimmed.starts_with('#')
    {
        return false;
    }
    let prefixes = [
        "pub ", "pub(", "async fn ", "fn ", "struct ", "enum ", "trait ", "impl ",
        "type ", "mod ", "def ", "async def ", "class ", "fun ", "suspend fun ",
        "override fun ", "object ", "companion object", "interface ", "export ",
        "async function ", "function ", "func ", "public ", "private ", "protected ",
        "internal ", "abstract ", "sealed ", "open ", "data class ",
    ];
    prefixes.iter().any(|p| trimmed.starts_with(p))
}

pub(crate) fn handle_symbols(proj_path: &Path, rel_path: &str) -> String {
    if rel_path.is_empty() {
        return "Error: relative_path is required for symbols/structure operation. Example: project_context(operation=\"symbols\", project_path=\"...\", relative_path=\"src/main.rs\")".to_string();
    }

    match validate_path(proj_path, rel_path) {
        Ok(full_path) => {
            if let Ok(content) = std::fs::read_to_string(&full_path) {
                let lang = AstLanguage::from_path(rel_path);
                let mut by_line: BTreeMap<usize, String> = BTreeMap::new();

                if lang.is_supported() {
                    for sym in AstEngine::extract_symbols(&content, lang) {
                        let sig_line = sym
                            .signature
                            .lines()
                            .next()
                            .unwrap_or(&sym.signature)
                            .trim();
                        let display = if sig_line.is_empty() {
                            format!("[{}] {}", sym.kind, sym.name)
                        } else {
                            format!("[{}] {}", sym.kind, sig_line)
                        };
                        by_line.entry(sym.start_line.max(1)).or_insert(display);
                    }
                    if lang == AstLanguage::Kotlin {
                        for (idx, line) in content.lines().enumerate() {
                            let trimmed = line.trim();
                            if trimmed.starts_with("companion object")
                                || trimmed.starts_with("object ")
                            {
                                by_line
                                    .entry(idx + 1)
                                    .or_insert_with(|| format!("[object] {}", trimmed));
                            }
                        }
                    }
                }

                if by_line.is_empty() {
                    for (line, label) in extract_symbols_via_db(proj_path, rel_path) {
                        by_line.entry(line).or_insert(label);
                    }
                }

                if by_line.is_empty() && !lang.is_supported() {
                    for (idx, line) in content.lines().enumerate() {
                        let trimmed = line.trim();
                        if is_modifier_symbol_line(trimmed) {
                            by_line.insert(idx + 1, trimmed.to_string());
                        }
                    }
                }

                let symbols: Vec<String> = by_line
                    .into_iter()
                    .map(|(line, sig)| format!("L{:04}: {}", line, sig))
                    .collect();

                format!(
                    "# Code Symbol Signatures: {}\n\n{}",
                    rel_path,
                    if symbols.is_empty() {
                        "No top-level symbol signatures found.".to_string()
                    } else {
                        symbols.join("\n")
                    }
                )
            } else {
                format!("Failed to read file '{}'", rel_path)
            }
        }
        Err(err_msg) => format!("Security Error: {}", err_msg),
    }
}

pub(crate) fn handle_references(
    arguments: &Value,
    proj_path: &Path,
    query: &str,
    rel_path: &str,
) -> String {
    if query.is_empty() && rel_path.is_empty() {
        return "Error: query symbol is required for references operation. Example: project_context(operation=\"references\", project_path=\"...\", query=\"MyStruct\")".to_string();
    }

    // Fast Path 1: Language Server Protocol (LSP) if line or relative_path is provided
    if let Some(lsp_resp) = super::context_lsp::try_lsp_references(arguments, proj_path, query, rel_path) {
        return lsp_resp;
    }

    let mut refs = Vec::new();
    let search_term = if query.is_empty() { rel_path } else { query };

    // Fast Path 2: Query SQLite CodeGraphDb FTS5 index (< 5ms)
    if let Ok(db) = CodeGraphDb::open_for_project(proj_path) {
        if let Ok(hits) = db.search_content_fts(search_term, 30) {
            let term_lower = search_term.to_lowercase();
            for (path, start_line, end_line, snippet) in hits {
                let mut reported_line = start_line;
                let mut reported_snip = snippet.lines().next().unwrap_or(&snippet).trim().to_string();
                if let Ok(full_file) = validate_path(proj_path, &path) {
                    if let Ok(file_content) = std::fs::read_to_string(&full_file) {
                        for (idx, line_text) in file_content.lines().enumerate() {
                            let line_no = idx + 1;
                            if line_no >= start_line && line_no <= end_line && line_text.to_lowercase().contains(&term_lower) {
                                reported_line = line_no;
                                reported_snip = line_text.trim().to_string();
                                break;
                            }
                        }
                    }
                }
                let bounded = if reported_snip.chars().count() > 100 {
                    crate::mcp::tools::helpers::truncate_chars(&reported_snip, 100).to_string()
                } else {
                    reported_snip
                };
                refs.push(format!("{}:L{} -> {}", path, reported_line, bounded));
                if refs.len() >= 30 {
                    break;
                }
            }
        }
    }

    // Fallback: Filesystem scan (depth 12) if DB is empty/uninitialized
    if refs.is_empty() {
        let files = scan_project(proj_path, 12);
        let query_lower = search_term.to_lowercase();
        for file in files.iter().filter(|f| f.file_type == "file") {
            if let Ok(path) = validate_path(proj_path, &file.path) {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    for (line_num, line) in content.lines().enumerate() {
                        if line.to_lowercase().contains(&query_lower) {
                            let trimmed = line.trim();
                            let bounded_line = if trimmed.chars().count() > 100 {
                                crate::mcp::tools::helpers::truncate_chars(trimmed, 100)
                            } else {
                                trimmed
                            };
                            refs.push(format!("{}:L{} -> {}", file.path, line_num + 1, bounded_line));
                            if refs.len() >= 30 {
                                break;
                            }
                        }
                    }
                }
            }
            if refs.len() >= 30 {
                break;
            }
        }
    }

    format!(
        "# Symbol References for '{}' (Max 30):\n\n{}",
        search_term,
        if refs.is_empty() {
            "No references found.".to_string()
        } else {
            refs.join("\n")
        }
    )
}

pub(crate) fn handle_learn_alias(
    arguments: &Value,
    proj_path: &Path,
    rel_path: &str,
) -> String {
    let alias_term = arguments.get("alias_term").and_then(|a| a.as_str()).unwrap_or("");
    let resolved_symbol = arguments.get("resolved_symbol").and_then(|s| s.as_str());
    let resolved_line = arguments.get("resolved_line").and_then(|l| l.as_u64()).map(|l| l as usize);

    if alias_term.is_empty() || rel_path.is_empty() {
        "Error: alias_term and relative_path are required for learn_alias. Example: project_context(operation=\"learn_alias\", project_path=\"...\", alias_term=\"login\", relative_path=\"src/auth/service.rs\")".to_string()
    } else if rel_path.contains("..") || rel_path.contains('\0') {
        "Security Error: Path traversal or null byte is strictly prohibited in relative_path.".to_string()
    } else {
        if proj_path.exists()
            && let Err(err_msg) = validate_path(proj_path, rel_path)
        {
            return format!("Security Error: {}", err_msg);
        }
        match CodeGraphDb::open_for_project(proj_path) {
            Ok(db) => match db.upsert_alias(alias_term, rel_path, resolved_symbol, resolved_line) {
                Ok(()) => {
                    format!(
                        "# Alias Learned Successfully\n\n- Alias Term: `{}`\n- Resolved Path: `{}`\n- Symbol: `{}`\n- Line: {}\n- Persistence: Saved in `.agent-context/code_graph.db`",
                        alias_term,
                        rel_path,
                        resolved_symbol.unwrap_or("—"),
                        resolved_line.map(|l| l.to_string()).unwrap_or_else(|| "—".to_string())
                    )
                }
                Err(e) => format!("Failed to record alias: {}", e),
            },
            Err(e) => format!("Failed to open code graph database: {}", e),
        }
    }
}
