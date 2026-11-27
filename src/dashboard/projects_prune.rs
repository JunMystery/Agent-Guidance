use anyhow::Result;
use rusqlite::{params, Connection, OpenFlags};
use std::path::Path;
use std::time::Duration;

use super::projects_path::{is_temp_project_path, normalize_project_path};

/// Returns true if a forward-slash, lowercase path points into an OS temporary directory
/// (never matching substrings like "template" or "temporary").
fn is_os_temp_dir(norm_slashes_lower: &str) -> bool {
    norm_slashes_lower.contains("/appdata/local/temp/")
        || norm_slashes_lower.ends_with("/appdata/local/temp")
        || norm_slashes_lower.starts_with("/tmp/")
        || norm_slashes_lower == "/tmp"
        || norm_slashes_lower.contains("/tmp/")
}

/// Returns true if a path belongs to an ephemeral test directory or OS temp directory
/// whose synthetic `tool_calls` rows should be deleted rather than preserved.
fn is_temp_test_tool_call_path(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return false;
    }
    let norm_slashes_lower = trimmed.to_ascii_lowercase().replace('\\', "/");

    for component in norm_slashes_lower.split('/') {
        if component.starts_with("ag_tools_test_")
            || component.starts_with("deep_search_test_")
            || component.starts_with("ag_graph_mcp_test_")
            || component.starts_with("ag_neighborhood_test_")
            || component.starts_with("last_project_path_test_")
        {
            return true;
        }
    }

    !cfg!(test) && is_os_temp_dir(&norm_slashes_lower)
}

/// Prunes projects whose directories no longer exist on the filesystem or are temp test folders.
pub fn prune_missing_projects(db_path: &Path) -> Result<usize> {
    if !db_path.exists() {
        return Ok(0);
    }

    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_FULL_MUTEX,
    )?;
    let _ = conn.busy_timeout(Duration::from_millis(5000));

    let mut stmt = conn.prepare("SELECT project_path FROM tracked_projects")?;
    let paths: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    let mut pruned_count = 0;
    for p in paths {
        if is_temp_project_path(&p) || !Path::new(&p).exists() {
            if let Ok(affected) = conn.execute("DELETE FROM tracked_projects WHERE project_path = ?", params![p]) {
                pruned_count += affected;
            }
            continue;
        }

        let norm = normalize_project_path(&p);
        if norm != p {
            let norm_exists: bool = conn
                .query_row(
                    "SELECT 1 FROM tracked_projects WHERE project_path = ?",
                    params![norm],
                    |_| Ok(true),
                )
                .unwrap_or(false);

            if norm_exists {
                let _ = conn.execute(
                    "UPDATE tracked_projects SET \
                        total_calls = total_calls + (SELECT total_calls FROM tracked_projects WHERE project_path = ?1), \
                        total_tokens_saved = total_tokens_saved + (SELECT total_tokens_saved FROM tracked_projects WHERE project_path = ?1), \
                        last_active = MAX(last_active, (SELECT last_active FROM tracked_projects WHERE project_path = ?1)) \
                     WHERE project_path = ?2",
                    params![p, norm],
                );
                let _ = conn.execute("DELETE FROM tracked_projects WHERE project_path = ?", params![p]);
                pruned_count += 1;
            } else {
                let root_name = Path::new(&norm).file_name().and_then(|n| n.to_str()).unwrap_or("project");
                let _ = conn.execute(
                    "UPDATE tracked_projects SET project_path = ?1, project_name = ?2 WHERE project_path = ?3",
                    params![norm, root_name, p],
                );
            }
            let _ = conn.execute(
                "UPDATE tool_calls SET project_path = ? WHERE project_path = ?",
                params![norm, p],
            );
        }
    }

    // Prune ephemeral temp test rows and normalize any legacy un-migrated paths in tool_calls
    if let Ok(mut stmt_tc) = conn.prepare("SELECT DISTINCT project_path FROM tool_calls WHERE project_path IS NOT NULL AND project_path != ''") {
        if let Ok(tc_paths) = stmt_tc.query_map([], |row| row.get::<_, String>(0)) {
            let paths: Vec<String> = tc_paths.filter_map(|r| r.ok()).collect();
            for p in paths {
                if is_temp_test_tool_call_path(&p) {
                    let _ = conn.execute("DELETE FROM tool_calls WHERE project_path = ?", params![p]);
                    continue;
                }
                if is_temp_project_path(&p) {
                    let _ = conn.execute("UPDATE tool_calls SET project_path = NULL WHERE project_path = ?", params![p]);
                    continue;
                }
                let norm = normalize_project_path(&p);
                if norm != p {
                    let _ = conn.execute("UPDATE tool_calls SET project_path = ? WHERE project_path = ?", params![norm, p]);
                }
            }
        }
    }

    Ok(pruned_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SEQ: AtomicU64 = AtomicU64::new(1);

    fn unique_test_dir(label: &str) -> std::path::PathBuf {
        let seq = TEST_SEQ.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "ag_prune_unit_{}_{}_{}",
            label,
            std::process::id(),
            seq
        ))
    }

    #[test]
    fn test_is_os_temp_dir_rejects_template_paths() {
        assert!(is_os_temp_dir("c:/users/dev/appdata/local/temp/run123"));
        assert!(is_os_temp_dir("c:/users/dev/appdata/local/temp"));
        assert!(is_os_temp_dir("/tmp/project_a"));
        assert!(is_os_temp_dir("/tmp"));
        assert!(!is_os_temp_dir("d:/git/nextjs-template"));
        assert!(!is_os_temp_dir("/home/user/templates"));
        assert!(!is_os_temp_dir("c:/projects/temporary-notes"));
    }

    #[test]
    fn test_prune_preserves_template_project_and_deletes_temp_test() {
        let base = unique_test_dir("preserve_template");
        let template_dir = base.join("my-nextjs-template");
        fs::create_dir_all(template_dir.join(".git")).unwrap();

        let db_path = base.join("usage.db");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE tracked_projects (
                project_path TEXT PRIMARY KEY,
                project_name TEXT NOT NULL,
                total_calls INTEGER NOT NULL DEFAULT 0,
                total_tokens_saved INTEGER NOT NULL DEFAULT 0,
                last_active INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE tool_calls (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                tool_name TEXT NOT NULL,
                started_at INTEGER NOT NULL,
                project_path TEXT
            );",
        )
        .unwrap();

        let real_template_path = normalize_project_path(&template_dir.to_string_lossy());
        let historical_template_path = "D:\\Git\\nextjs-template";
        let temp_test_path = base.join("ag_tools_test_123").to_string_lossy().to_string();

        conn.execute(
            "INSERT INTO tracked_projects (project_path, project_name, total_calls, total_tokens_saved, last_active)
             VALUES (?1, 'my-nextjs-template', 5, 1000, 100)",
            params![real_template_path],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tracked_projects (project_path, project_name, total_calls, total_tokens_saved, last_active)
             VALUES (?1, 'ag_tools_test_123', 2, 50, 100)",
            params![temp_test_path],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO tool_calls (tool_name, started_at, project_path) VALUES ('project_context', 100, ?1)",
            params![real_template_path],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tool_calls (tool_name, started_at, project_path) VALUES ('project_context', 101, ?1)",
            params![historical_template_path],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO tool_calls (tool_name, started_at, project_path) VALUES ('project_context', 102, ?1)",
            params![temp_test_path],
        )
        .unwrap();
        drop(conn);

        let pruned = prune_missing_projects(&db_path).unwrap();
        assert_eq!(pruned, 1);

        let verify_conn = Connection::open(&db_path).unwrap();
        let tracked_template: i64 = verify_conn
            .query_row(
                "SELECT COUNT(*) FROM tracked_projects WHERE project_path = ?1",
                params![real_template_path],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tracked_template, 1, "Real template project must stay in tracked_projects");

        let tracked_test: i64 = verify_conn
            .query_row(
                "SELECT COUNT(*) FROM tracked_projects WHERE project_path = ?1",
                params![temp_test_path],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tracked_test, 0, "ag_tools_test_123 must be pruned from tracked_projects");

        let tc_real: i64 = verify_conn
            .query_row(
                "SELECT COUNT(*) FROM tool_calls WHERE project_path = ?1",
                params![real_template_path],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tc_real, 1, "tool_calls for existing template project must be preserved");

        let tc_hist: i64 = verify_conn
            .query_row(
                "SELECT COUNT(*) FROM tool_calls WHERE project_path LIKE '%nextjs-template%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tc_hist, 2, "tool_calls for non-temp template projects must be preserved");

        let tc_test: i64 = verify_conn
            .query_row(
                "SELECT COUNT(*) FROM tool_calls WHERE project_path = ?1",
                params![temp_test_path],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tc_test, 0, "tool_calls for ag_tools_test_123 must be deleted");

        drop(verify_conn);
        let _ = fs::remove_dir_all(&base);
    }
}
