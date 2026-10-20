use super::*;
use rusqlite::Connection;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE tool_calls (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            tool_name TEXT NOT NULL,
            operation TEXT,
            started_at INTEGER NOT NULL,
            duration_ms INTEGER,
            tokens_original INTEGER DEFAULT 0,
            tokens_optimized INTEGER DEFAULT 0,
            error_message TEXT,
            project_path TEXT,
            target TEXT
        );
        CREATE TABLE skill_loads (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            skill_id TEXT NOT NULL,
            loaded_at INTEGER NOT NULL
        );
        CREATE TABLE embed_queries (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            query_text TEXT NOT NULL,
            queried_at INTEGER NOT NULL
        );
        CREATE TABLE daily_summaries (
            day TEXT PRIMARY KEY,
            tool_calls INTEGER DEFAULT 0,
            skills_loaded INTEGER DEFAULT 0,
            embed_queries INTEGER DEFAULT 0,
            llm_queries INTEGER DEFAULT 0,
            tokens_original INTEGER DEFAULT 0,
            tokens_optimized INTEGER DEFAULT 0
        );
        CREATE TABLE dashboard_daily_rollups (
            day TEXT NOT NULL,
            project_path TEXT NOT NULL,
            tool_name TEXT NOT NULL,
            operation TEXT NOT NULL,
            phase TEXT NOT NULL,
            calls INTEGER NOT NULL DEFAULT 0,
            tokens_original INTEGER NOT NULL DEFAULT 0,
            tokens_optimized INTEGER NOT NULL DEFAULT 0,
            total_duration_ms INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (day, project_path, tool_name, operation, phase)
        );
        CREATE TABLE daily_skill_rollups (
            day TEXT NOT NULL,
            project_path TEXT NOT NULL,
            skill_id TEXT NOT NULL,
            calls INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (day, project_path, skill_id)
        );
        CREATE TABLE mcp_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp INTEGER NOT NULL,
            level TEXT NOT NULL,
            source TEXT NOT NULL,
            message TEXT NOT NULL,
            details TEXT,
            project_path TEXT
        );"
    ).unwrap();
    conn
}

#[test]
fn test_rollup_before_prune_aggregation() {
    let conn = setup_test_db();
    let cutoff = 1773187200; // test epoch
    let past = cutoff - 86400 * 5;

    conn.execute(
        "INSERT INTO tool_calls (tool_name, operation, started_at, duration_ms, tokens_original, tokens_optimized, project_path)
         VALUES ('task_pipeline', 'plan', ?1, 50, 1000, 200, 'test_proj')",
        [past],
    ).unwrap();
    conn.execute(
        "INSERT INTO tool_calls (tool_name, operation, started_at, duration_ms, tokens_original, tokens_optimized, project_path)
         VALUES ('task_pipeline', 'plan', ?1, 70, 500, 100, 'test_proj')",
        [past],
    ).unwrap();
    conn.execute(
        "INSERT INTO skill_loads (skill_id, loaded_at) VALUES ('rust-expert', ?1)",
        [past],
    ).unwrap();

    let count = rollup_before_prune(&conn, cutoff).unwrap();
    assert_eq!(count, 1);

    let (calls, orig, opt, dur): (i64, i64, i64, i64) = conn.query_row(
        "SELECT calls, tokens_original, tokens_optimized, total_duration_ms FROM dashboard_daily_rollups WHERE project_path = 'test_proj'",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)? , r.get(3)?)),
    ).unwrap();

    assert_eq!(calls, 2);
    assert_eq!(orig, 1500);
    assert_eq!(opt, 300);
    assert_eq!(dur, 120);

    let skill_calls: i64 = conn.query_row(
        "SELECT calls FROM daily_skill_rollups WHERE skill_id = 'rust-expert'",
        [],
        |r| r.get(0),
    ).unwrap();
    assert_eq!(skill_calls, 1);
}

#[test]
fn test_clean_dashboard_history_project_isolation() {
    let conn = setup_test_db();
    conn.execute(
        "INSERT INTO dashboard_daily_rollups (day, project_path, tool_name, operation, phase, calls)
         VALUES ('2026-03-01', 'proj_a', 'task_pipeline', 'plan', 'plan', 5)",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO dashboard_daily_rollups (day, project_path, tool_name, operation, phase, calls)
         VALUES ('2026-03-01', 'proj_b', 'task_pipeline', 'plan', 'plan', 3)",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO mcp_logs (timestamp, level, source, message) VALUES (12345, 'INFO', 'test', 'should remain')",
        [],
    ).unwrap();

    let cleaned = clean_dashboard_history(&conn, Some("proj_a")).unwrap();
    assert!(cleaned > 0);

    let proj_a_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM dashboard_daily_rollups WHERE project_path = 'proj_a'",
        [],
        |r| r.get(0),
    ).unwrap();
    assert_eq!(proj_a_count, 0);

    let proj_b_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM dashboard_daily_rollups WHERE project_path = 'proj_b'",
        [],
        |r| r.get(0),
    ).unwrap();
    assert_eq!(proj_b_count, 1);

    // Verify mcp_logs are completely untouched
    let logs_count: i64 = conn.query_row("SELECT COUNT(*) FROM mcp_logs", [], |r| r.get(0)).unwrap();
    assert_eq!(logs_count, 1);
}
