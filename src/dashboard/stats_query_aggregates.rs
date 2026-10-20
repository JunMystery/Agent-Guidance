use rusqlite::{params, Connection};
use serde_json::{json, Value};

#[allow(dead_code)]
pub fn query_hourly_savings(conn: &Connection, now: i64, is_proj: bool, p1: &str, p2: &str) -> Vec<Value> {
    super::stats_chart_query::query_adaptive_savings(conn, now, "past_24h", is_proj, p1, p2)
}

pub fn query_phase_and_gov(conn: &Connection, cutoff: i64, is_proj: bool, p1: &str, p2: &str) -> (Value, Value) {
    let sql = if is_proj {
        "SELECT tool_name, LOWER(COALESCE(operation, '')), LOWER(COALESCE(target, '')), COUNT(*) FROM tool_calls WHERE started_at >= ?1 AND (tool_name = 'task_pipeline' OR tool_name = 'workflow_gate' OR (tool_name = 'guidance' AND operation = 'verify')) AND (project_path = ?2 COLLATE NOCASE OR project_path = ?3 COLLATE NOCASE OR rtrim(project_path, '/\\') = ?2 COLLATE NOCASE OR rtrim(project_path, '/\\') = ?3 COLLATE NOCASE) GROUP BY tool_name, LOWER(COALESCE(operation, '')), LOWER(COALESCE(target, ''))"
    } else {
        "SELECT tool_name, LOWER(COALESCE(operation, '')), LOWER(COALESCE(target, '')), COUNT(*) FROM tool_calls WHERE started_at >= ?1 AND (tool_name = 'task_pipeline' OR tool_name = 'workflow_gate' OR (tool_name = 'guidance' AND operation = 'verify')) GROUP BY tool_name, LOWER(COALESCE(operation, '')), LOWER(COALESCE(target, ''))"
    };
    let mut phases = json!({ "plan": 0, "build": 0, "test": 0, "fix": 0, "review": 0, "refactor": 0 });
    let mut gov = json!({ "edits_authorized": 0, "plans_approved": 0, "verifications_passed": 0, "stages_transitioned": 0 });

    let mut add_phase = |key: &str, cnt: i64| {
        if let Some(v) = phases.get_mut(key) {
            *v = json!(v.as_i64().unwrap_or(0) + cnt);
        }
    };

    if let Ok(mut stmt) = conn.prepare(sql) {
        let mapper = |r: &rusqlite::Row| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?));
        let rows = if is_proj { stmt.query_map(params![cutoff, p1, p2], mapper) }
                   else { stmt.query_map([cutoff], mapper) };
        if let Ok(iter) = rows {
            for (t, op, target, cnt) in iter.flatten() {
                if t == "task_pipeline" {
                    match op.as_str() {
                        "plan" | "architecture" | "design" | "init" => add_phase("plan", cnt),
                        "build" | "implement" | "code" | "create" => add_phase("build", cnt),
                        "test" | "verify" | "test_recheck" | "testing" => add_phase("test", cnt),
                        "fix" | "debug" | "bugfix" | "patch" => add_phase("fix", cnt),
                        "review" | "audit" => add_phase("review", cnt),
                        "refactor" | "decompose" => add_phase("refactor", cnt),
                        other => add_phase(other, cnt),
                    }
                } else if t == "workflow_gate" {
                    if op == "authorize_edit" {
                        gov["edits_authorized"] = json!(gov["edits_authorized"].as_i64().unwrap_or(0) + cnt);
                        add_phase("build", cnt);
                    } else if op == "approve_plan" || op == "approve" {
                        gov["plans_approved"] = json!(gov["plans_approved"].as_i64().unwrap_or(0) + cnt);
                        add_phase("plan", cnt);
                    } else if op == "verify" || op == "pass_verification" {
                        gov["verifications_passed"] = json!(gov["verifications_passed"].as_i64().unwrap_or(0) + cnt);
                        add_phase("test", cnt);
                    } else if op == "set_stage" || op == "advance" {
                        gov["stages_transitioned"] = json!(gov["stages_transitioned"].as_i64().unwrap_or(0) + cnt);
                        if target.contains("build") {
                            add_phase("build", cnt);
                        } else if target.contains("test") {
                            add_phase("test", cnt);
                        } else if target.contains("fix") {
                            add_phase("fix", cnt);
                        } else if target.contains("review") || target.contains("proposal") {
                            add_phase("review", cnt);
                        } else if target.contains("plan") || target.contains("context") {
                            add_phase("plan", cnt);
                        }
                    }
                } else if t == "guidance" && op == "verify" {
                    gov["verifications_passed"] = json!(gov["verifications_passed"].as_i64().unwrap_or(0) + cnt);
                    add_phase("test", cnt);
                }
            }
        }
    }

    let r_sql = if is_proj {
        "SELECT phase, tool_name, operation, SUM(calls) FROM dashboard_daily_rollups WHERE CAST(strftime('%s', day) AS INTEGER) >= ?1 AND (project_path = ?2 COLLATE NOCASE OR project_path = ?3 COLLATE NOCASE OR rtrim(project_path, '/\\') = ?2 COLLATE NOCASE OR rtrim(project_path, '/\\') = ?3 COLLATE NOCASE) GROUP BY phase, tool_name, operation"
    } else {
        "SELECT phase, tool_name, operation, SUM(calls) FROM dashboard_daily_rollups WHERE CAST(strftime('%s', day) AS INTEGER) >= ?1 GROUP BY phase, tool_name, operation"
    };
    if let Ok(mut r_stmt) = conn.prepare(r_sql) {
        let mapper = |r: &rusqlite::Row| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?));
        let rows = if is_proj { r_stmt.query_map(params![cutoff, p1, p2], mapper) } else { r_stmt.query_map([cutoff], mapper) };
        if let Ok(iter) = rows {
            for (phase, tool_name, op, cnt) in iter.flatten() {
                add_phase(&phase, cnt);
                if tool_name == "workflow_gate" {
                    if op == "authorize_edit" { gov["edits_authorized"] = json!(gov["edits_authorized"].as_i64().unwrap_or(0) + cnt); }
                    else if op == "approve_plan" || op == "approve" { gov["plans_approved"] = json!(gov["plans_approved"].as_i64().unwrap_or(0) + cnt); }
                    else if op == "verify" || op == "pass_verification" { gov["verifications_passed"] = json!(gov["verifications_passed"].as_i64().unwrap_or(0) + cnt); }
                } else if tool_name == "guidance" && op == "verify" {
                    gov["verifications_passed"] = json!(gov["verifications_passed"].as_i64().unwrap_or(0) + cnt);
                }
            }
        }
    }
    (phases, gov)
}

pub fn query_timeframe_summary(conn: &Connection, cutoff: i64, is_proj: bool, p1: &str, p2: &str) -> Value {
    let (calls, orig, opt): (i64, i64, i64) = if is_proj {
        conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(tokens_original), 0), COALESCE(SUM(tokens_optimized), 0)
             FROM tool_calls WHERE started_at >= ?1 AND tool_name != 'mcp_tool' AND (project_path = ?2 COLLATE NOCASE OR project_path = ?3 COLLATE NOCASE OR rtrim(project_path, '/\\') = ?2 COLLATE NOCASE OR rtrim(project_path, '/\\') = ?3 COLLATE NOCASE)",
            params![cutoff, p1, p2],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ).unwrap_or((0, 0, 0))
    } else {
        conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(tokens_original), 0), COALESCE(SUM(tokens_optimized), 0)
             FROM tool_calls WHERE started_at >= ?1 AND tool_name != 'mcp_tool'",
            params![cutoff],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ).unwrap_or((0, 0, 0))
    };

    let (ctx_calls, ctx_orig, ctx_opt): (i64, i64, i64) = if is_proj {
        conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(tokens_original), 0), COALESCE(SUM(tokens_optimized), 0)
             FROM tool_calls WHERE started_at >= ?1 AND tool_name = 'project_context' AND (project_path = ?2 COLLATE NOCASE OR project_path = ?3 COLLATE NOCASE OR rtrim(project_path, '/\\') = ?2 COLLATE NOCASE OR rtrim(project_path, '/\\') = ?3 COLLATE NOCASE)",
            params![cutoff, p1, p2],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ).unwrap_or((0, 0, 0))
    } else {
        conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(tokens_original), 0), COALESCE(SUM(tokens_optimized), 0)
             FROM tool_calls WHERE started_at >= ?1 AND tool_name = 'project_context'",
            params![cutoff],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ).unwrap_or((0, 0, 0))
    };

    let (skills_cnt, r_skills): (i64, i64) = (
        conn.query_row("SELECT COUNT(*) FROM skill_loads WHERE loaded_at >= ?1", [cutoff], |r| r.get(0)).unwrap_or(0),
        conn.query_row("SELECT COALESCE(SUM(calls), 0) FROM daily_skill_rollups WHERE CAST(strftime('%s', day) AS INTEGER) >= ?1", [cutoff], |r| r.get(0)).unwrap_or(0),
    );
    let skills = skills_cnt + r_skills;

    let (r_calls, r_orig, r_opt): (i64, i64, i64) = if is_proj {
        conn.query_row(
            "SELECT COALESCE(SUM(calls), 0), COALESCE(SUM(tokens_original), 0), COALESCE(SUM(tokens_optimized), 0)
             FROM dashboard_daily_rollups
             WHERE CAST(strftime('%s', day) AS INTEGER) >= ?1 AND (project_path = ?2 COLLATE NOCASE OR project_path = ?3 COLLATE NOCASE OR rtrim(project_path, '/\\') = ?2 COLLATE NOCASE OR rtrim(project_path, '/\\') = ?3 COLLATE NOCASE)",
            params![cutoff, p1, p2],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ).unwrap_or((0, 0, 0))
    } else {
        conn.query_row(
            "SELECT COALESCE(SUM(calls), 0), COALESCE(SUM(tokens_original), 0), COALESCE(SUM(tokens_optimized), 0)
             FROM dashboard_daily_rollups
             WHERE CAST(strftime('%s', day) AS INTEGER) >= ?1",
            params![cutoff],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ).unwrap_or((0, 0, 0))
    };
    let calls = calls + r_calls;
    let orig = orig + r_orig;
    let opt = opt + r_opt;

    let (r_ctx_calls, r_ctx_orig, r_ctx_opt): (i64, i64, i64) = if is_proj {
        conn.query_row(
            "SELECT COALESCE(SUM(calls), 0), COALESCE(SUM(tokens_original), 0), COALESCE(SUM(tokens_optimized), 0)
             FROM dashboard_daily_rollups
             WHERE tool_name = 'project_context' AND CAST(strftime('%s', day) AS INTEGER) >= ?1 AND (project_path = ?2 COLLATE NOCASE OR project_path = ?3 COLLATE NOCASE OR rtrim(project_path, '/\\') = ?2 COLLATE NOCASE OR rtrim(project_path, '/\\') = ?3 COLLATE NOCASE)",
            params![cutoff, p1, p2],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ).unwrap_or((0, 0, 0))
    } else {
        conn.query_row(
            "SELECT COALESCE(SUM(calls), 0), COALESCE(SUM(tokens_original), 0), COALESCE(SUM(tokens_optimized), 0)
             FROM dashboard_daily_rollups
             WHERE tool_name = 'project_context' AND CAST(strftime('%s', day) AS INTEGER) >= ?1",
            params![cutoff],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ).unwrap_or((0, 0, 0))
    };
    let ctx_calls = ctx_calls + r_ctx_calls;
    let ctx_orig = ctx_orig + r_ctx_orig;
    let ctx_opt = ctx_opt + r_ctx_opt;

    let embeds: i64 = conn.query_row("SELECT COUNT(*) FROM embed_queries WHERE queried_at >= ?1", [cutoff], |r| r.get(0)).unwrap_or(0);
    let saved = (orig - opt).max(0);
    let pct = if orig > 0 { (((saved as f64 / orig as f64) * 1000.0).round() / 10.0).max(0.0) } else { 0.0 };
    let graph_precision_pct = if ctx_orig > 0 {
        ((((ctx_orig - ctx_opt).max(0) as f64 / ctx_orig as f64) * 1000.0).round() / 10.0).max(0.0)
    } else {
        pct
    };

    json!({
        "tool_calls": calls, "skills_loaded": skills, "embed_queries": embeds, "llm_queries": 0,
        "tokens_original": orig, "tokens_optimized": opt, "token_savings": saved, "savings_pct": pct,
        "graph_precision_pct": graph_precision_pct, "context_calls": ctx_calls,
        "context_tokens_orig": ctx_orig, "context_tokens_opt": ctx_opt
    })
}

pub fn query_summaries(conn: &Connection, now: i64, cutoff_24h: i64, is_proj: bool, p1: &str, p2: &str) -> Value {
    json!({
        "past_24h": query_timeframe_summary(conn, cutoff_24h, is_proj, p1, p2),
        "last_7d": query_timeframe_summary(conn, now.saturating_sub(7 * 86400), is_proj, p1, p2),
        "last_30d": query_timeframe_summary(conn, now.saturating_sub(30 * 86400), is_proj, p1, p2),
        "lifetime": query_timeframe_summary(conn, 0, is_proj, p1, p2),
    })
}

#[cfg(test)]
#[path = "stats_query_aggregates_tests.rs"]
mod tests;
