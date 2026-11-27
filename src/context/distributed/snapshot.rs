//! Graph snapshot export and import with integrity verification.
#![allow(dead_code)]

use anyhow::{bail, Context, Result};
use rusqlite::Connection;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use crate::context::db::CodeGraphDb;

static SNAPSHOT_TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Computes a deterministic SHA-256 checksum of a file on disk.
pub fn compute_file_checksum(file_path: &Path) -> Result<String> {
    crate::mcp::fingerprint::compute_file_sha256(file_path)
        .with_context(|| format!("Cannot compute SHA-256 checksum for: {:?}", file_path))
}

/// Exports the SQLite code graph database of `project_path` into `out_file`.
/// Returns the snapshot checksum.
pub fn export_graph_snapshot(project_path: &Path, out_file: &Path) -> Result<String> {
    let db_path = project_path.join(".agent-context").join("code_graph.db");
    if !db_path.exists() {
        bail!("Code graph database does not exist at {:?}", db_path);
    }

    // Flush WAL to ensure complete snapshot
    if let Ok(db) = CodeGraphDb::open_for_project(project_path) {
        let _ = db.conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    }

    if let Some(parent) = out_file.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::copy(&db_path, out_file)
        .with_context(|| format!("Failed to copy snapshot from {:?} to {:?}", db_path, out_file))?;

    compute_file_checksum(out_file)
}

/// Imports and verifies a snapshot into `project_path`, replacing existing database.
pub fn import_graph_snapshot(
    snapshot_file: &Path,
    project_path: &Path,
    expected_checksum: Option<&str>,
) -> Result<()> {
    if !snapshot_file.exists() {
        bail!("Snapshot file not found: {:?}", snapshot_file);
    }

    // 1. Verify Checksum
    let actual_checksum = compute_file_checksum(snapshot_file)?;
    if let Some(expected) = expected_checksum {
        if actual_checksum != expected {
            bail!(
                "Snapshot checksum mismatch: expected {}, got {}",
                expected,
                actual_checksum
            );
        }
    }

    // 2. Copy into unique staging file so caller's snapshot stays untouched, then verify SQLite integrity
    let agent_ctx = project_path.join(".agent-context");
    fs::create_dir_all(&agent_ctx)?;
    let target_db = agent_ctx.join("code_graph.db");

    let seq = SNAPSHOT_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let temp_target = agent_ctx.join(format!("code_graph.db.incoming.{}.{}", std::process::id(), seq));
    fs::copy(snapshot_file, &temp_target)?;

    let verify_res: Result<()> = (|| {
        let verify_conn = Connection::open_with_flags(
            &temp_target,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        verify_conn.execute_batch("PRAGMA trusted_schema = OFF;")?;
        let integrity: String = verify_conn.query_row("PRAGMA quick_check;", [], |r| r.get(0))?;
        if integrity != "ok" {
            bail!("Snapshot SQLite integrity check failed: {}", integrity);
        }
        Ok(())
    })();
    if let Err(e) = verify_res {
        let _ = fs::remove_file(&temp_target);
        return Err(e);
    }

    // 3. Atomically install into target directory
    if let Err(e) = fs::rename(&temp_target, &target_db) {
        let _ = fs::remove_file(&temp_target);
        return Err(e.into());
    }

    // Remove old WAL and SHM if present
    let _ = fs::remove_file(agent_ctx.join("code_graph.db-wal"));
    let _ = fs::remove_file(agent_ctx.join("code_graph.db-shm"));

    Ok(())
}
