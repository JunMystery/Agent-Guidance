use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use super::ServerState;

static TMP_FILE_SEQ: AtomicU64 = AtomicU64::new(1);
static SAVE_CLEANUP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Atomically writes `content` to `path` via a unique sibling temp file and rename.
pub(crate) fn atomic_write_file(path: &Path, content: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("state");
    let seq = TMP_FILE_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp_path = path.with_file_name(format!("{}.tmp.{}.{}", file_name, std::process::id(), seq));
    if let Err(e) = fs::write(&tmp_path, content) {
        let _ = fs::remove_file(&tmp_path);
        return Err(e);
    }
    if let Err(e) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(e);
    }
    Ok(())
}

impl ServerState {
    pub fn cleanup_stale_sessions(proj_path: &Path) {
        let dir = proj_path.join(".agent-context").join("sessions");
        if !dir.exists() {
            return;
        }

        let now = SystemTime::now();
        let max_age = std::time::Duration::from_secs(30 * 86400); // 30 Days Retention
        let mut entries = Vec::new();

        if let Ok(read_dir) = fs::read_dir(&dir) {
            for entry in read_dir.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("json") {
                    if let Ok(metadata) = entry.metadata() {
                        let modified = metadata.modified().unwrap_or(now);
                        if now.duration_since(modified).unwrap_or_default() > max_age {
                            let _ = fs::remove_file(&path);
                            continue;
                        }
                        entries.push((modified, path));
                    }
                }
            }
        }

        // LRU Cap: If > 100 session files, delete oldest
        if entries.len() > 100 {
            entries.sort_by_key(|(mtime, _)| *mtime);
            for (_, path) in entries.iter().take(entries.len() - 100) {
                let _ = fs::remove_file(path);
            }
        }
    }

    /// Validates a session/conversation identifier against path traversal and unsafe filename characters.
    pub fn is_valid_session_id(id: &str) -> bool {
        !id.is_empty()
            && id.len() <= 128
            && !id.starts_with('.')
            && !id.ends_with('.')
            && !id.contains("..")
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    }

    pub fn save_to_dir(&self, proj_path: &Path) -> Result<(), String> {
        if !Self::is_valid_session_id(&self.session_id) {
            return Err(format!("Invalid session_id '{}'", self.session_id));
        }
        crate::mcp::impact::ensure_agent_context_gitignored(proj_path);
        let dir = proj_path.join(".agent-context").join("sessions");
        if let Err(e) = fs::create_dir_all(&dir) {
            return Err(format!("Failed to create directory: {}", e));
        }
        let file_path = dir.join(format!("{}.json", self.session_id));
        let content = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;

        // Atomic write via unique temp file + rename
        atomic_write_file(&file_path, content.as_bytes()).map_err(|e| e.to_string())?;

        // Also atomically update legacy single-session pointer
        let legacy_file = proj_path.join(".agent-context").join("session.json");
        let _ = atomic_write_file(&legacy_file, content.as_bytes());

        if SAVE_CLEANUP_SEQ.fetch_add(1, Ordering::Relaxed).is_multiple_of(16) {
            Self::cleanup_stale_sessions(proj_path);
        }
        Ok(())
    }

    /// Automatically saves snapshot checkpoint to `.agent-context/sessions/{session_id}.json`.
    pub fn auto_checkpoint(&self, proj_path: &Path) -> Result<(), String> {
        self.save_to_dir(proj_path)
    }

    pub fn load_from_dir(proj_path: &Path) -> Result<Self, String> {
        Self::cleanup_stale_sessions(proj_path);
        crate::mcp::snapshots::cleanup_stale_snapshots(proj_path, 7, 20);
        let dir = proj_path.join(".agent-context").join("sessions");

        // 1. Try finding most recent session file in sessions/
        if dir.exists() {
            if let Ok(read_dir) = fs::read_dir(&dir) {
                let mut session_files: Vec<(SystemTime, std::path::PathBuf)> = read_dir
                    .flatten()
                    .filter_map(|e| {
                        let p = e.path();
                        if p.extension().and_then(|s| s.to_str()) == Some("json") {
                            let mtime = e.metadata().ok()?.modified().ok()?;
                            Some((mtime, p))
                        } else {
                            None
                        }
                    })
                    .collect();

                session_files.sort_by_key(|(mtime, _)| *mtime);
                for (_, newest_path) in session_files.iter().rev() {
                    if let Ok(content) = fs::read_to_string(newest_path) {
                        if let Ok(loaded) = serde_json::from_str::<Self>(&content) {
                            return Ok(loaded);
                        }
                    }
                }
            }
        }

        // 2. Legacy fallback: .agent-context/session.json
        let legacy_file = proj_path.join(".agent-context").join("session.json");
        if legacy_file.exists() {
            if let Ok(content) = fs::read_to_string(&legacy_file) {
                if let Ok(loaded) = serde_json::from_str::<Self>(&content) {
                    return Ok(loaded);
                }
            }
        }

        Ok(Self::new())
    }

    /// Lists all active/saved sessions from `.agent-context/sessions/`.
    pub fn list_sessions(proj_path: &Path) -> Vec<Self> {
        let dir = proj_path.join(".agent-context").join("sessions");
        if !dir.exists() {
            return Vec::new();
        }

        let Ok(read_dir) = fs::read_dir(&dir) else {
            return Vec::new();
        };

        let mut sessions: Vec<(SystemTime, Self)> = read_dir
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                if p.extension().and_then(|s| s.to_str()) == Some("json") {
                    let mtime = e.metadata().ok()?.modified().ok()?;
                    let content = fs::read_to_string(&p).ok()?;
                    let loaded = serde_json::from_str::<Self>(&content).ok()?;
                    Some((mtime, loaded))
                } else {
                    None
                }
            })
            .collect();

        sessions.sort_by_key(|(mtime, _)| *mtime);
        sessions.into_iter().rev().map(|(_, s)| s).collect()
    }

    /// Loads a specific session by its session_id.
    pub fn load_session_by_id(proj_path: &Path, target_id: &str) -> Result<Self, String> {
        if !Self::is_valid_session_id(target_id) {
            return Err(format!("Invalid session_id '{}'", target_id));
        }
        let dir = proj_path.join(".agent-context").join("sessions");
        let file_path = dir.join(format!("{}.json", target_id));
        if !file_path.exists() {
            return Err(format!(
                "Session '{}' not found in '{}'. Use operation=\"list\" to view available sessions.",
                target_id,
                dir.display()
            ));
        }

        let content = fs::read_to_string(&file_path).map_err(|e| e.to_string())?;
        serde_json::from_str::<Self>(&content).map_err(|e| format!("Corrupted session file '{}': {}", target_id, e))
    }

    pub fn global_project_path_file() -> std::path::PathBuf {
        if cfg!(test) {
            let tid = format!("{:?}", std::thread::current().id()).replace(['(', ')'], "");
            std::env::temp_dir().join(format!(
                "last_project_path_test_{}_{}",
                std::process::id(),
                tid
            ))
        } else {
            dirs::home_dir()
                .map(|h| h.join(".agent-guidance").join("last_project_path"))
                .unwrap_or_else(|| std::path::PathBuf::from(".last_project_path"))
        }
    }

    pub fn read_global_project_path() -> Option<String> {
        let path = Self::global_project_path_file();
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                let trimmed = content.trim();
                if !trimmed.is_empty() && Path::new(trimmed).is_dir() {
                    return Some(trimmed.to_string());
                }
            }
        }
        None
    }

    pub fn update_project_path(&mut self, path: &Path) {
        let path_str = path.to_string_lossy().to_string();
        if path_str.is_empty() || path_str == "." {
            return;
        }
        if self.project_path.as_deref() == Some(path_str.as_str()) {
            return;
        }
        if !self.workspace_roots.is_empty() && !self.is_path_inside_workspace(path) {
            return;
        }
        if let Some(ref prev) = self.project_path {
            let can_prev = Path::new(prev).canonicalize().unwrap_or_else(|_| Path::new(prev).to_path_buf());
            let can_new = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
            if can_prev != can_new {
                self.reset_for_context_switch();
            }
        }
        self.project_path = Some(path_str.clone());

        let global_file = Self::global_project_path_file();
        let _ = atomic_write_file(&global_file, path_str.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_id_rejects_trailing_dot() {
        assert!(!ServerState::is_valid_session_id("session."));
        assert!(!ServerState::is_valid_session_id(".session"));
        assert!(ServerState::is_valid_session_id("session_v1.2"));
    }

    #[test]
    fn test_load_from_dir_skips_corrupted_newest_session() {
        let dir = std::env::temp_dir().join(format!(
            "ag_corrupt_fallback_{}_{}",
            std::process::id(),
            TMP_FILE_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let mut valid = ServerState::new();
        valid.session_id = "valid_session".to_string();
        valid.workflow_stage = "Build".to_string();
        valid.save_to_dir(&dir).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(20));
        let corrupt_path = dir
            .join(".agent-context")
            .join("sessions")
            .join("zzz_corrupt.json");
        fs::write(&corrupt_path, "{ invalid json").unwrap();

        let loaded = ServerState::load_from_dir(&dir).unwrap();
        assert_eq!(loaded.session_id, "valid_session");
        assert_eq!(loaded.workflow_stage, "Build");

        // Ensure no leftover .tmp.* files remain in sessions/
        let leftovers: Vec<_> = fs::read_dir(dir.join(".agent-context").join("sessions"))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
            .collect();
        assert!(leftovers.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }
}
