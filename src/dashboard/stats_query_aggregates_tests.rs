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
        );"
    ).unwrap();
    conn
}

#[test]
fn test_query_hourly_savings_metrics_and_is_current() {
    let conn = setup_test_db();
    let now = 1773187200; // test epoch
    conn.execute(
        "INSERT INTO tool_calls (tool_name, operation, started_at, duration_ms, tokens_original, tokens_optimized, project_path)
         VALUES ('project_context', 'read', ?1, 120, 1000, 200, 'test_proj')",
        [now],
    ).unwrap();

    let res = query_hourly_savings(&conn, now, false, "", "");
    assert_eq!(res.len(), 24);

    let last = &res[23];
    assert_eq!(last["is_current"], true);
    assert_eq!(last["calls"], 1);
    assert_eq!(last["avg_duration_ms"], 120);
    assert_eq!(last["tokens_optimized"], 200);
    assert_eq!(last["optimized"], 200);
    assert!(last["timestamp"].is_number());

    let first = &res[0];
    assert_eq!(first["is_current"], false);
    assert_eq!(first["calls"], 0);
}

#[test]
fn test_query_phase_and_gov_lifecycle_cadence() {
    let conn = setup_test_db();
    let now = 1773187200;
    conn.execute("INSERT INTO tool_calls (tool_name, operation, started_at, target) VALUES ('task_pipeline', 'plan', ?1, '')", [now]).unwrap();
    conn.execute("INSERT INTO tool_calls (tool_name, operation, started_at, target) VALUES ('task_pipeline', 'debug', ?1, '')", [now]).unwrap();
    conn.execute("INSERT INTO tool_calls (tool_name, operation, started_at, target) VALUES ('workflow_gate', 'authorize_edit', ?1, 'src/main.rs')", [now]).unwrap();
    conn.execute("INSERT INTO tool_calls (tool_name, operation, started_at, target) VALUES ('guidance', 'verify', ?1, '')", [now]).unwrap();
    conn.execute("INSERT INTO tool_calls (tool_name, operation, started_at, target) VALUES ('workflow_gate', 'advance', ?1, 'Review')", [now]).unwrap();

    let (phases, gov) = query_phase_and_gov(&conn, now - 3600, false, "", "");
    assert_eq!(phases["plan"], 1);
    assert_eq!(phases["fix"], 1);
    assert_eq!(phases["build"], 1);
    assert_eq!(phases["test"], 1);
    assert_eq!(phases["review"], 1);
    assert_eq!(gov["edits_authorized"], 1);
    assert_eq!(gov["verifications_passed"], 1);
    assert_eq!(gov["stages_transitioned"], 1);
}

#[test]
fn test_query_timeframe_summary_graph_precision() {
    let conn = setup_test_db();
    let now = 1773187200;
    conn.execute(
        "INSERT INTO tool_calls (tool_name, operation, started_at, duration_ms, tokens_original, tokens_optimized, project_path)
         VALUES ('project_context', 'read', ?1, 100, 1000, 200, 'test_proj')",
        [now],
    ).unwrap();

    conn.execute(
        "INSERT INTO dashboard_daily_rollups (day, project_path, tool_name, operation, phase, calls, tokens_original, tokens_optimized, total_duration_ms)
         VALUES ('2026-03-01', 'test_proj', 'project_context', 'read', 'build', 2, 2000, 400, 200)",
        [],
    ).unwrap();

    let summary = query_timeframe_summary(&conn, 0, false, "", "");
    assert_eq!(summary["tool_calls"], 3);
    assert_eq!(summary["tokens_original"], 3000);
    assert_eq!(summary["tokens_optimized"], 600);
    assert_eq!(summary["token_savings"], 2400);
}
