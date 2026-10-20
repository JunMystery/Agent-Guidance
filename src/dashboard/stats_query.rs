use anyhow::Result;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;

use super::stats_query_aggregates::{query_phase_and_gov, query_summaries};

pub fn query_usage_stats(db_path: &Path, proj_filter: Option<&str>, timeframe: &str) -> Result<Value> {
    if !db_path.exists() {
        return Ok(json!({ "db_status": "missing" }));
    }

    let conn = Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_FULL_MUTEX,
    )?;
    ensure_tables(&conn);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let (normalized_tf, cutoff) = match timeframe {
        "last_7d" | "7d" => ("last_7d", now.saturating_sub(7 * 86400)),
        "last_30d" | "30d" => ("last_30d", now.saturating_sub(30 * 86400)),
        "lifetime" | "all" => ("lifetime", 0),
        _ => ("past_24h", now.saturating_sub(86400)),
    };

    let is_proj = proj_filter.is_some() && proj_filter != Some("all");
    let p1 = proj_filter.unwrap_or("").replace('\\', "/");
    let p2 = proj_filter.unwrap_or("").replace('/', "\\");

    // 1. Tool breakdown
    let mut tb_map: HashMap<(String, Option<String>), (i64, i64, i64, i64)> = HashMap::new();
    let tool_sql = if is_proj {
        "SELECT tool_name, operation, COUNT(*), COALESCE(SUM(tokens_original), 0), COALESCE(SUM(tokens_optimized), 0), COALESCE(SUM(duration_ms), 0)
         FROM tool_calls WHERE started_at >= ?1 AND tool_name != 'mcp_tool' AND (project_path = ?2 COLLATE NOCASE OR project_path = ?3 COLLATE NOCASE OR rtrim(project_path, '/\\') = ?2 COLLATE NOCASE OR rtrim(project_path, '/\\') = ?3 COLLATE NOCASE) GROUP BY tool_name, operation"
    } else {
        "SELECT tool_name, operation, COUNT(*), COALESCE(SUM(tokens_original), 0), COALESCE(SUM(tokens_optimized), 0), COALESCE(SUM(duration_ms), 0)
         FROM tool_calls WHERE started_at >= ?1 AND tool_name != 'mcp_tool' GROUP BY tool_name, operation"
    };
    if let Ok(mut stmt) = conn.prepare(tool_sql) {
        let mapper = |r: &rusqlite::Row| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?, r.get::<_, i64>(4)?, r.get::<_, i64>(5)?));
        let rows = if is_proj { stmt.query_map(params![cutoff, p1, p2], mapper) } else { stmt.query_map([cutoff], mapper) };
        if let Ok(iter) = rows {
            for (t, op, cnt, orig, opt, dur) in iter.flatten() {
                let e = tb_map.entry((t, op)).or_default();
                e.0 += cnt; e.1 += orig; e.2 += opt; e.3 += dur;
            }
        }
    }
    let r_tool_sql = if is_proj {
        "SELECT tool_name, operation, SUM(calls), SUM(tokens_original), SUM(tokens_optimized), SUM(total_duration_ms)
         FROM dashboard_daily_rollups WHERE CAST(strftime('%s', day) AS INTEGER) >= ?1 AND (project_path = ?2 COLLATE NOCASE OR project_path = ?3 COLLATE NOCASE OR rtrim(project_path, '/\\') = ?2 COLLATE NOCASE OR rtrim(project_path, '/\\') = ?3 COLLATE NOCASE) GROUP BY tool_name, operation"
    } else {
        "SELECT tool_name, operation, SUM(calls), SUM(tokens_original), SUM(tokens_optimized), SUM(total_duration_ms)
         FROM dashboard_daily_rollups WHERE CAST(strftime('%s', day) AS INTEGER) >= ?1 GROUP BY tool_name, operation"
    };
    if let Ok(mut r_stmt) = conn.prepare(r_tool_sql) {
        let mapper = |r: &rusqlite::Row| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?, r.get::<_, i64>(4)?, r.get::<_, i64>(5)?));
        let rows = if is_proj { r_stmt.query_map(params![cutoff, p1, p2], mapper) } else { r_stmt.query_map([cutoff], mapper) };
        if let Ok(iter) = rows {
            for (t, op, cnt, orig, opt, dur) in iter.flatten() {
                let e = tb_map.entry((t, op)).or_default();
                e.0 += cnt; e.1 += orig; e.2 += opt; e.3 += dur;
            }
        }
    }
    let tool_breakdown: Vec<Value> = tb_map.into_iter().map(|((tool_name, operation), (cnt, orig, opt, dur))| {
        let avg_dur = if cnt > 0 { dur as f64 / cnt as f64 } else { 0.0 };
        json!({
            "tool_name": tool_name,
            "operation": operation,
            "count": cnt,
            "tokens_original": orig,
            "tokens_optimized": opt,
            "avg_duration_ms": avg_dur
        })
    }).collect();

    // 2. Top Skills
    let mut top_skills_map: HashMap<String, i64> = HashMap::new();
    let mut stmt = conn.prepare("SELECT skill_id, COUNT(*) FROM skill_loads WHERE loaded_at >= ? GROUP BY skill_id")?;
    let rows = stmt.query_map([cutoff], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?;
    for item in rows.flatten() {
        if let Some(clean_id) = normalize_skill_name(&item.0) {
            *top_skills_map.entry(clean_id).or_default() += item.1;
        }
    }
    let r_skill_sql = if is_proj {
        "SELECT skill_id, SUM(calls) FROM daily_skill_rollups WHERE CAST(strftime('%s', day) AS INTEGER) >= ?1 AND (project_path = ?2 COLLATE NOCASE OR project_path = ?3 COLLATE NOCASE OR project_path = '') GROUP BY skill_id"
    } else {
        "SELECT skill_id, SUM(calls) FROM daily_skill_rollups WHERE CAST(strftime('%s', day) AS INTEGER) >= ?1 GROUP BY skill_id"
    };
    if let Ok(mut r_stmt) = conn.prepare(r_skill_sql) {
        let mapper = |r: &rusqlite::Row| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?));
        let rows = if is_proj { r_stmt.query_map(params![cutoff, p1, p2], mapper) } else { r_stmt.query_map([cutoff], mapper) };
        if let Ok(iter) = rows {
            for (sk, cnt) in iter.flatten() {
                if let Some(clean_id) = normalize_skill_name(&sk) {
                    *top_skills_map.entry(clean_id).or_default() += cnt;
                }
            }
        }
    }
    let tc_sql = if is_proj {
        "SELECT operation, COUNT(*) FROM tool_calls WHERE started_at >= ?1 AND (tool_name = 'select_skills' OR tool_name = 'select_skill') AND operation IS NOT NULL AND operation != 'none' AND (project_path = ?2 COLLATE NOCASE OR project_path = ?3 COLLATE NOCASE OR rtrim(project_path, '/\\') = ?2 COLLATE NOCASE OR rtrim(project_path, '/\\') = ?3 COLLATE NOCASE) GROUP BY operation"
    } else {
        "SELECT operation, COUNT(*) FROM tool_calls WHERE started_at >= ?1 AND (tool_name = 'select_skills' OR tool_name = 'select_skill') AND operation IS NOT NULL AND operation != 'none' GROUP BY operation"
    };
    if let Ok(mut tc_stmt) = conn.prepare(tc_sql) {
        let mapper = |r: &rusqlite::Row| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?));
        let tc_rows = if is_proj {
            tc_stmt.query_map(params![cutoff, p1, p2], mapper)
        } else {
            tc_stmt.query_map([cutoff], mapper)
        };
        if let Ok(iter) = tc_rows {
            for (ops, tc_cnt) in iter.flatten() {
                for single in ops.split(',') {
                    if let Some(clean_id) = normalize_skill_name(single) {
                        let e = top_skills_map.entry(clean_id).or_default();
                        *e = (*e).max(tc_cnt);
                    }
                }
            }
        }
    }
    let mut top_skills: Vec<Value> = top_skills_map
        .into_iter()
        .map(|(k, v)| json!({ "skill_id": k, "cnt": v }))
        .collect();
    top_skills.sort_by(|a, b| b["cnt"].as_i64().unwrap_or(0).cmp(&a["cnt"].as_i64().unwrap_or(0)));
    top_skills.truncate(50);

    let mut stmt = conn.prepare("SELECT skill_id, loaded_at FROM skill_loads ORDER BY loaded_at DESC LIMIT 100")?;
    let mut recent_skill_calls: Vec<Value> = Vec::new();
    let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?;
    for item in rows.flatten() {
        if let Some(clean_id) = normalize_skill_name(&item.0) {
            recent_skill_calls.push(json!({ "skill_id": clean_id, "loaded_at": item.1 }));
            if recent_skill_calls.len() >= 50 { break; }
        }
    }
    if recent_skill_calls.len() < 50 {
        let tc_rec = "SELECT operation, started_at FROM tool_calls WHERE (tool_name = 'select_skills' OR tool_name = 'select_skill') AND operation IS NOT NULL AND operation != 'none' ORDER BY started_at DESC LIMIT 50";
        if let Ok(mut stmt) = conn.prepare(tc_rec) {
            if let Ok(rows) = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))) {
                for (op, ts) in rows.flatten() {
                    for single in op.split(',') {
                        if let Some(clean_id) = normalize_skill_name(single) {
                            if !recent_skill_calls.iter().any(|v| v["skill_id"] == clean_id && v["loaded_at"] == ts) {
                                recent_skill_calls.push(json!({ "skill_id": clean_id, "loaded_at": ts }));
                            }
                        }
                    }
                }
                recent_skill_calls.sort_by(|a, b| b["loaded_at"].as_i64().unwrap_or(0).cmp(&a["loaded_at"].as_i64().unwrap_or(0)));
                recent_skill_calls.truncate(50);
            }
        }
    }

    // 3. Recent Actions
    let recent_sql = if is_proj {
        "SELECT tool_name, operation, started_at, duration_ms, tokens_original, tokens_optimized, error_message, target FROM tool_calls WHERE started_at >= ?1 AND tool_name != 'mcp_tool' AND (project_path = ?2 COLLATE NOCASE OR project_path = ?3 COLLATE NOCASE OR rtrim(project_path, '/\\') = ?2 COLLATE NOCASE OR rtrim(project_path, '/\\') = ?3 COLLATE NOCASE) ORDER BY started_at DESC LIMIT 50"
    } else {
        "SELECT tool_name, operation, started_at, duration_ms, tokens_original, tokens_optimized, error_message, target FROM tool_calls WHERE started_at >= ?1 AND tool_name != 'mcp_tool' ORDER BY started_at DESC LIMIT 50"
    };
    let mut stmt = conn.prepare(recent_sql)?;
    let recent_actions: Vec<Value> = if is_proj {
        stmt.query_map(params![cutoff, p1, p2], map_recent_action)?
    } else {
        stmt.query_map([cutoff], map_recent_action)?
    }
    .filter_map(|r| r.ok())
    .collect();

    // 4. Hourly Savings & Summaries
    let hourly_savings = super::stats_chart_query::query_adaptive_savings(&conn, now, normalized_tf, is_proj, &p1, &p2);
    let summaries = query_summaries(&conn, now, now.saturating_sub(86400), is_proj, &p1, &p2);

    let mut stmt = conn.prepare("SELECT query_text, queried_at FROM embed_queries WHERE queried_at >= ? ORDER BY queried_at DESC LIMIT 50")?;
    let embed_recent: Vec<Value> = stmt.query_map([cutoff], |row| {
        Ok(json!({ "query": row.get::<_, String>(0)?, "created_at": row.get::<_, i64>(1)? }))
    })?.filter_map(|r| r.ok()).collect();

    let active_totals = summaries.get(normalized_tf).cloned().unwrap_or_else(|| summaries["past_24h"].clone());
    let (phase_stats, governance_stats) = query_phase_and_gov(&conn, cutoff, is_proj, &p1, &p2);

    let remote_skills = if crate::config::current_config().server.is_remote() {
        crate::client::cache::get_cached_remote_stats().map(|s| {
            json!({
                "mode": "remote",
                "total_skills": s.total_skills,
                "total_sections": s.total_sections,
                "catalog_hash": s.catalog_hash,
                "token_savings_ratio": s.token_savings_ratio,
                "last_reindex_at": s.last_reindex_at
            })
        })
    } else {
        None
    };

    Ok(json!({
        "db_status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "project_path": proj_filter.unwrap_or("all"),
        "timeframe": normalized_tf,
        "server_port": crate::dashboard::get_dashboard_port(),
        "totals": active_totals,
        "summaries": summaries,
        "tool_breakdown": tool_breakdown,
        "top_skills": top_skills,
        "recent_skill_calls": recent_skill_calls,
        "recent_actions": recent_actions,
        "hourly_savings": hourly_savings,
        "phase_stats": phase_stats,
        "governance_stats": governance_stats,
        "embed_recent": embed_recent,
        "remote_skills": remote_skills
    }))
}

fn map_recent_action(row: &rusqlite::Row) -> rusqlite::Result<Value> {
    Ok(json!({
        "tool_name": row.get::<_, String>(0)?,
        "operation": row.get::<_, Option<String>>(1)?,
        "started_at": row.get::<_, i64>(2)?,
        "duration_ms": row.get::<_, Option<i64>>(3)?,
        "tokens_original": row.get::<_, Option<i64>>(4)?,
        "tokens_optimized": row.get::<_, Option<i64>>(5)?,
        "error_message": row.get::<_, Option<String>>(6)?,
        "target": row.get::<_, Option<String>>(7)?
    }))
}

fn normalize_skill_name(raw: &str) -> Option<String> {
    let clean = crate::mcp::tools::skills::clean_skill_identifier(raw);
    if clean.is_empty() {
        None
    } else {
        Some(clean)
    }
}

fn ensure_tables(conn: &Connection) {
    let _ = conn.execute("CREATE TABLE IF NOT EXISTS tool_calls (id INTEGER PRIMARY KEY AUTOINCREMENT, tool_name TEXT NOT NULL, operation TEXT, started_at INTEGER NOT NULL, duration_ms INTEGER, tokens_original INTEGER DEFAULT 0, tokens_optimized INTEGER DEFAULT 0, error_message TEXT, project_path TEXT, target TEXT)", []);
    let _ = conn.execute("ALTER TABLE tool_calls ADD COLUMN target TEXT", []);
    let _ = conn.execute("CREATE TABLE IF NOT EXISTS skill_loads (id INTEGER PRIMARY KEY AUTOINCREMENT, skill_id TEXT NOT NULL, loaded_at INTEGER NOT NULL)", []);
    let _ = conn.execute("CREATE TABLE IF NOT EXISTS embed_queries (id INTEGER PRIMARY KEY AUTOINCREMENT, query_text TEXT NOT NULL, queried_at INTEGER NOT NULL)", []);
    let _ = conn.execute("DELETE FROM tool_calls WHERE tool_name = 'mcp_tool'", []);
}
