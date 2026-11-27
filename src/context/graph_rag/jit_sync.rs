use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use anyhow::Result;
use crate::context::indexer::{IncrementalIndexer, IndexReport};

static PROJECT_LAST_SYNC: OnceLock<RwLock<HashMap<PathBuf, u64>>> = OnceLock::new();
static INDEXING_IN_PROGRESS: OnceLock<RwLock<HashSet<PathBuf>>> = OnceLock::new();

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Returns true if background GraphRAG indexing is currently running for this project.
pub fn is_indexing(project_path: &Path) -> bool {
    let canonical = project_path.canonicalize().unwrap_or_else(|_| project_path.to_path_buf());
    let in_progress_lock = INDEXING_IN_PROGRESS.get_or_init(|| RwLock::new(HashSet::new()));
    let prog_guard = in_progress_lock.read().unwrap_or_else(|p| p.into_inner());
    prog_guard.contains(&canonical)
}

/// Detects whether the project has never been indexed before (cold start).
fn is_cold_start(project_path: &Path) -> bool {
    let db_path = project_path.join(".agent-context").join("code_graph.db");
    if !db_path.exists() {
        return true;
    }
    match crate::context::db::CodeGraphDb::open_read_only(&db_path) {
        Ok(db) => db.file_count().map(|c| c == 0).unwrap_or(true),
        Err(_) => true,
    }
}

struct IndexingGuard {
    canonical: PathBuf,
}

impl IndexingGuard {
    fn try_acquire(canonical: PathBuf) -> Option<Self> {
        let in_progress_lock = INDEXING_IN_PROGRESS.get_or_init(|| RwLock::new(HashSet::new()));
        let mut prog_guard = in_progress_lock.write().unwrap_or_else(|p| p.into_inner());
        if !prog_guard.insert(canonical.clone()) {
            return None;
        }
        Some(Self { canonical })
    }
}

impl Drop for IndexingGuard {
    fn drop(&mut self) {
        let in_progress_lock = INDEXING_IN_PROGRESS.get_or_init(|| RwLock::new(HashSet::new()));
        let mut prog_guard = in_progress_lock.write().unwrap_or_else(|p| p.into_inner());
        prog_guard.remove(&self.canonical);
    }
}

/// Ensures the project code graph and index are fresh before serving GraphRAG / Context requests.
/// Debounced by `debounce_secs` individually per project path so multiple IDEs/projects do not block each other.
/// Uses fast batch transactions for disk writes, while heavy embeddings and GraphRAG run in the background.
/// Cold start indexing is offloaded to a background thread to prevent MCP timeout on fresh projects.
pub fn ensure_fresh_graph(project_path: &Path, debounce_secs: u64) -> Result<Option<IndexReport>> {
    let canonical = project_path.canonicalize().unwrap_or_else(|_| project_path.to_path_buf());
    let now = now_secs();

    // Atomically check-and-insert into INDEXING_IN_PROGRESS under a single write lock.
    // The RAII guard guarantees cleanup on early return, spawn failure, or panic.
    let guard = match IndexingGuard::try_acquire(canonical.clone()) {
        Some(g) => g,
        None => return Ok(None),
    };

    let map_lock = PROJECT_LAST_SYNC.get_or_init(|| RwLock::new(HashMap::new()));
    {
        let mut write_guard = map_lock.write().unwrap_or_else(|p| p.into_inner());
        if let Some(&last) = write_guard.get(&canonical)
            && now.saturating_sub(last) < debounce_secs
        {
            return Ok(None);
        }
        write_guard.insert(canonical.clone(), now);
    }

    // Cold-start non-blocking guard: on a brand new project, full AST indexing,
    // cross-edge extraction and RAG embedding run in a background thread to prevent MCP timeout.
    if is_cold_start(&canonical) {
        let _ = crate::context::db::CodeGraphDb::open_for_project(&canonical);
        let path_clone = canonical.clone();
        let _ = std::thread::Builder::new()
            .name("ag-cold-start-indexer".to_string())
            .spawn(move || {
                let _guard = guard;
                if let Ok(mut indexer) = IncrementalIndexer::new(&path_clone) {
                    if let Ok(report) = indexer.incremental_index() {
                        if report.files_indexed > 0 {
                            let _ = indexer.embed_symbols();
                            let _ = indexer.embed_chunks();
                            let engine = super::GraphRagEngine::new(&path_clone);
                            let _ = engine.build_or_update("Auto");
                        }
                    }
                }
            });
        return Ok(None);
    }

    let mut indexer = match IncrementalIndexer::new(project_path) {
        Ok(idx) => idx,
        Err(_) => return Ok(None),
    };

    let report_res = indexer.incremental_index();
    drop(guard);

    let report = report_res?;
    if report.files_indexed > 0 {
        // Trigger background Rayon embedding pool and update community hierarchy
        let path_clone = project_path.to_path_buf();
        std::thread::spawn(move || {
            if let Ok(idx) = IncrementalIndexer::new(&path_clone) {
                let _ = idx.embed_symbols();
                let _ = idx.embed_chunks();
            }
            let engine = super::GraphRagEngine::new(&path_clone);
            let _ = engine.build_or_update("Auto");
        });
        return Ok(Some(report));
    }

    Ok(None)
}

use crate::context::indexer::invalidator::{FileInvalidationReport, invalidate_and_sync_file};

pub fn ensure_fresh_targeted_file(
    project_path: &Path,
    rel_path: &str,
) -> Result<Option<FileInvalidationReport>> {
    let clean = rel_path.trim().replace('\\', "/");
    if clean.is_empty() {
        return Ok(None);
    }
    let report = invalidate_and_sync_file(project_path, &clean)?;
    if report.reindexed {
        Ok(Some(report))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_isolated_test_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ag_jit_sync_{}_{}_{}",
            label,
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src").join("lib.rs"), "pub fn ping() -> i32 { 1 }\n").unwrap();
        dir
    }

    fn wait_indexing_idle(dir: &Path) {
        for _ in 0..100 {
            if !is_indexing(dir) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn test_debounce_guard() {
        let p = make_isolated_test_dir("debounce");
        let res1 = ensure_fresh_graph(&p, 100);
        assert!(res1.is_ok());
        wait_indexing_idle(&p);
        // Immediate second call after first completes must be skipped by debounce
        let res2 = ensure_fresh_graph(&p, 100);
        assert!(res2.is_ok());
        assert!(res2.unwrap().is_none());

        wait_indexing_idle(&p);
        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn test_distinct_projects_do_not_block_each_other() {
        let p1 = make_isolated_test_dir("distinct_a");
        let p2 = make_isolated_test_dir("distinct_b");
        let res1 = ensure_fresh_graph(&p1, 100);
        assert!(res1.is_ok());
        let res2 = ensure_fresh_graph(&p2, 100);
        assert!(res2.is_ok());

        wait_indexing_idle(&p1);
        wait_indexing_idle(&p2);
        let _ = std::fs::remove_dir_all(&p1);
        let _ = std::fs::remove_dir_all(&p2);
    }

    #[test]
    fn test_indexing_guard_atomic_and_panic_safe() {
        let p = make_isolated_test_dir("guard_raii");
        let canonical = p.canonicalize().unwrap_or_else(|_| p.clone());

        let first = IndexingGuard::try_acquire(canonical.clone());
        assert!(first.is_some());
        assert!(is_indexing(&p));
        // Second acquire while held must fail atomically
        assert!(IndexingGuard::try_acquire(canonical.clone()).is_none());

        // Panic while holding the guard must still release INDEXING_IN_PROGRESS
        let panic_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _held = first;
            panic!("simulated indexing panic");
        }));
        assert!(panic_res.is_err());
        assert!(!is_indexing(&p));

        // Re-acquiring after drop/panic succeeds
        let reacquired = IndexingGuard::try_acquire(canonical);
        assert!(reacquired.is_some());
        drop(reacquired);
        assert!(!is_indexing(&p));

        let _ = std::fs::remove_dir_all(&p);
    }

    #[test]
    fn test_cold_start_non_blocking() {
        let temp_dir = std::env::temp_dir().join(format!("ag_cold_start_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        let _ = std::fs::create_dir_all(temp_dir.join("src"));
        std::fs::write(temp_dir.join("src").join("main.rs"), "fn main() {}").unwrap();

        assert!(is_cold_start(&temp_dir));
        let res = ensure_fresh_graph(&temp_dir, 0);
        assert!(res.is_ok());
        assert!(res.unwrap().is_none());

        wait_indexing_idle(&temp_dir);
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
