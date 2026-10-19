use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub fn query_adaptive_savings(
    conn: &Connection,
    now: i64,
    timeframe: &str,
    is_proj: bool,
    p1: &str,
    p2: &str,
) -> Vec<Value> {
    let (step, num_buckets, start_slot) = match timeframe {
        "last_7d" => {
            let step = 86400i64;
            let num = 7usize;
            let start = ((now - (6 * step)) / step) * step;
            (step, num, start)
        }
        "last_30d" => {
            let step = 86400i64;
            let num = 30usize;
            let start = ((now - (29 * step)) / step) * step;
            (step, num, start)
        }
        "lifetime" | "all" => {
            let min_sql = if is_proj {
                "SELECT MIN(started_at) FROM tool_calls WHERE started_at > 0 AND tool_name != 'mcp_tool' AND (project_path = ?1 COLLATE NOCASE OR project_path = ?2 COLLATE NOCASE OR rtrim(project_path, '/\\') = ?1 COLLATE NOCASE OR rtrim(project_path, '/\\') = ?2 COLLATE NOCASE)"
            } else {
                "SELECT MIN(started_at) FROM tool_calls WHERE started_at > 0 AND tool_name != 'mcp_tool'"
            };
            let earliest: Option<i64> = if is_proj {
                conn.query_row(min_sql, params![p1, p2], |r| r.get(0)).ok()
            } else {
                conn.query_row(min_sql, [], |r| r.get(0)).ok()
            };
            let earliest = earliest.unwrap_or(now - (29 * 86400));
            let span = (now - earliest).max(86400);
            if span > 30 * 86400 {
                let num = 30usize;
                let step = (((span / 29) + 86399) / 86400) * 86400;
                let start = ((now - (29 * step)) / step) * step;
                (step, num, start)
            } else {
                let step = 86400i64;
                let num = 30usize;
                let start = ((now - (29 * step)) / step) * step;
                (step, num, start)
            }
        }
        _ => {
            // Default: past_24h
            let step = 3600i64;
            let num = 24usize;
            let start = ((now - (23 * step)) / step) * step;
            (step, num, start)
        }
    };

    let mut slot_map = BTreeMap::new();
    for i in 0..num_buckets {
        let s = start_slot + ((i as i64) * step);
        slot_map.insert(s, (0i64, 0i64, 0i64, 0.0f64));
    }

    let sql = if is_proj {
        format!(
            "SELECT (started_at / {step}) * {step} as slot, COUNT(*), COALESCE(SUM(tokens_original), 0), COALESCE(SUM(tokens_optimized), 0), COALESCE(AVG(duration_ms), 0.0)
             FROM tool_calls WHERE started_at >= ?1 AND tool_name != 'mcp_tool' AND (project_path = ?2 COLLATE NOCASE OR project_path = ?3 COLLATE NOCASE OR rtrim(project_path, '/\\') = ?2 COLLATE NOCASE OR rtrim(project_path, '/\\') = ?3 COLLATE NOCASE) GROUP BY slot"
        )
    } else {
        format!(
            "SELECT (started_at / {step}) * {step} as slot, COUNT(*), COALESCE(SUM(tokens_original), 0), COALESCE(SUM(tokens_optimized), 0), COALESCE(AVG(duration_ms), 0.0)
             FROM tool_calls WHERE started_at >= ?1 AND tool_name != 'mcp_tool' GROUP BY slot"
        )
    };

    if let Ok(mut stmt) = conn.prepare(&sql) {
        let mapper = |row: &rusqlite::Row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, f64>(4)?,
            ))
        };
        let rows = if is_proj {
            stmt.query_map(params![start_slot, p1, p2], mapper)
        } else {
            stmt.query_map([start_slot], mapper)
        };
        if let Ok(iter) = rows {
            for (slot, cnt, orig, opt, dur) in iter.flatten() {
                slot_map.insert(slot, (cnt, orig, opt, dur));
            }
        }
    }

    let total_len = slot_map.len();
    slot_map
        .into_iter()
        .enumerate()
        .map(|(idx, (slot, (cnt, orig, opt, dur)))| {
            let saved = orig - opt;
            let is_current = idx + 1 == total_len;
            json!({
                "hour": slot,
                "timestamp": slot,
                "step": step,
                "calls": cnt,
                "avg_duration_ms": dur.round() as i64,
                "tokens_original": orig,
                "tokens_optimized": opt,
                "tokens_saved": saved,
                "original": orig,
                "optimized": opt,
                "saved": saved,
                "is_current": is_current
            })
        })
        .collect()
}
