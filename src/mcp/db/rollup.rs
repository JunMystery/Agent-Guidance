use anyhow::Result;
use rusqlite::{Connection, params};

pub fn rollup_before_prune(conn: &Connection, cutoff_ts: i64) -> Result<usize> {
    let mut stmt = conn.prepare(
        "SELECT
            strftime('%Y-%m-%d', started_at, 'unixepoch') AS day,
            COALESCE(project_path, '') AS proj,
            tool_name,
            COALESCE(operation, '') AS op,
            COUNT(*) AS cnt,
            COALESCE(SUM(tokens_original), 0) AS orig,
            COALESCE(SUM(tokens_optimized), 0) AS opt,
            COALESCE(SUM(duration_ms), 0) AS dur
         FROM tool_calls
         WHERE started_at < ?1 AND tool_name != 'mcp_tool'
         GROUP BY day, proj, tool_name, op",
    )?;

    let rows: Vec<(String, String, String, String, i64, i64, i64, i64)> = stmt
        .query_map([cutoff_ts], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();

    let mut inserted_count = 0;
    for (day, proj, tool_name, op, cnt, orig, opt, dur) in rows {
        let norm_proj = proj.replace('\\', "/");
        let phase = map_phase(&tool_name, &op);
        conn.execute(
            "INSERT INTO dashboard_daily_rollups
             (day, project_path, tool_name, operation, phase, calls, tokens_original, tokens_optimized, total_duration_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(day, project_path, tool_name, operation, phase) DO UPDATE SET
                 calls = calls + excluded.calls,
                 tokens_original = tokens_original + excluded.tokens_original,
                 tokens_optimized = tokens_optimized + excluded.tokens_optimized,
                 total_duration_ms = total_duration_ms + excluded.total_duration_ms",
            params![day, norm_proj, tool_name, op, phase, cnt, orig, opt, dur],
        )?;
        inserted_count += 1;
    }

    // Rollup skill loads from skill_loads table
    let mut skill_stmt = conn.prepare(
        "SELECT
            strftime('%Y-%m-%d', loaded_at, 'unixepoch') AS day,
            skill_id,
            COUNT(*) AS cnt
         FROM skill_loads
         WHERE loaded_at < ?1
         GROUP BY day, skill_id",
    )?;
    let skill_rows: Vec<(String, String, i64)> = skill_stmt
        .query_map([cutoff_ts], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .filter_map(|r| r.ok())
        .collect();

    for (day, skill_id, cnt) in skill_rows {
        let _ = conn.execute(
            "INSERT INTO daily_skill_rollups (day, project_path, skill_id, calls)
             VALUES (?1, '', ?2, ?3)
             ON CONFLICT(day, project_path, skill_id) DO UPDATE SET
                 calls = calls + excluded.calls",
            params![day, skill_id, cnt],
        );
    }

    // Rollup project-specific skills from select_skills tool_calls
    let tc_skill_sql = "SELECT strftime('%Y-%m-%d', started_at, 'unixepoch') AS day, COALESCE(project_path, '') AS proj, operation, COUNT(*) FROM tool_calls WHERE started_at < ?1 AND (tool_name = 'select_skills' OR tool_name = 'select_skill') AND operation IS NOT NULL AND operation != 'none' GROUP BY day, proj, operation";
    if let Ok(mut tc_stmt) = conn.prepare(tc_skill_sql) {
        if let Ok(rows) = tc_stmt.query_map([cutoff_ts], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?))) {
            for (day, proj, op, cnt) in rows.flatten() {
                let norm_proj = proj.replace('\\', "/");
                for single in op.split(',') {
                    let clean = crate::mcp::tools::skills::clean_skill_identifier(single);
                    if !clean.is_empty() {
                        let _ = conn.execute(
                            "INSERT INTO daily_skill_rollups (day, project_path, skill_id, calls)
                             VALUES (?1, ?2, ?3, ?4)
                             ON CONFLICT(day, project_path, skill_id) DO UPDATE SET
                                 calls = calls + excluded.calls",
                            params![day, norm_proj, clean, cnt],
                        );
                    }
                }
            }
        }
    }

    Ok(inserted_count)
}

fn map_phase(tool_name: &str, op: &str) -> String {
    let lower_op = op.to_ascii_lowercase();
    match tool_name {
        "task_pipeline" => match lower_op.as_str() {
            "plan" | "architecture" | "design" | "init" => "plan".to_string(),
            "build" | "implement" | "code" | "create" => "build".to_string(),
            "test" | "verify" | "test_recheck" | "testing" => "test".to_string(),
            "fix" | "debug" | "bugfix" | "patch" => "fix".to_string(),
            "review" | "audit" => "review".to_string(),
            "refactor" | "decompose" => "refactor".to_string(),
            other => other.to_string(),
        },
        "workflow_gate" => match lower_op.as_str() {
            "authorize_edit" => "build".to_string(),
            "approve_plan" | "approve" => "plan".to_string(),
            "verify" | "pass_verification" => "test".to_string(),
            _ => "gov".to_string(),
        },
        "guidance" if lower_op == "verify" => "test".to_string(),
        _ => "other".to_string(),
    }
}

pub fn clean_dashboard_history(conn: &Connection, project_filter: Option<&str>) -> Result<usize> {
    let is_all = project_filter.is_none() || project_filter == Some("all") || project_filter == Some("");
    let mut deleted = 0;

    if is_all {
        deleted += conn.execute("DELETE FROM dashboard_daily_rollups", [])?;
        deleted += conn.execute("DELETE FROM daily_skill_rollups", [])?;
        deleted += conn.execute("DELETE FROM tool_calls", [])?;
        deleted += conn.execute("DELETE FROM skill_loads", [])?;
        deleted += conn.execute("DELETE FROM embed_queries", [])?;
        deleted += conn.execute("DELETE FROM daily_summaries", [])?;
    } else {
        let p = project_filter.unwrap_or_default();
        let p_norm = p.replace('\\', "/");
        let p_win = p.replace('/', "\\");
        deleted += conn.execute(
            "DELETE FROM dashboard_daily_rollups
             WHERE project_path = ?1 COLLATE NOCASE OR project_path = ?2 COLLATE NOCASE
                OR rtrim(project_path, '/\\') = ?1 COLLATE NOCASE OR rtrim(project_path, '/\\') = ?2 COLLATE NOCASE",
            params![p_norm, p_win],
        )?;
        deleted += conn.execute(
            "DELETE FROM tool_calls
             WHERE project_path = ?1 COLLATE NOCASE OR project_path = ?2 COLLATE NOCASE
                OR rtrim(project_path, '/\\') = ?1 COLLATE NOCASE OR rtrim(project_path, '/\\') = ?2 COLLATE NOCASE",
            params![p_norm, p_win],
        )?;
        deleted += conn.execute(
            "DELETE FROM daily_skill_rollups
             WHERE project_path = ?1 COLLATE NOCASE OR project_path = ?2 COLLATE NOCASE",
            params![p_norm, p_win],
        )?;
    }

    Ok(deleted)
}
