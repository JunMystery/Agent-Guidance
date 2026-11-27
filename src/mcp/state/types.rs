use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{path::Path, sync::Arc, sync::atomic::{AtomicBool, AtomicU64, Ordering}, time::{SystemTime, UNIX_EPOCH}};
use tracing::info;

pub const SESSION_STALE_TIMEOUT_SECS: u64 = 300;
static SESSION_SEQ: AtomicU64 = AtomicU64::new(1);

pub use super::uri::parse_file_uri;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerState {
    #[serde(default = "generate_session_id")]
    pub session_id: String,
    #[serde(default)]
    pub priority_gate_passed: bool,
    #[serde(default = "default_workflow_stage")]
    pub workflow_stage: String,
    #[serde(default)]
    pub plan_approved: bool,
    #[serde(default)]
    pub fix_attempts: u32,
    #[serde(default)]
    pub tool_calls: u32,
    #[serde(default)]
    pub tokens_original: u64,
    #[serde(default)]
    pub tokens_optimized: u64,
    #[serde(default)]
    pub last_raw_baseline_tokens: Option<u64>,
    #[serde(default)]
    pub project_path: Option<String>,
    #[serde(default)]
    pub agent_client_name: Option<String>,
    #[serde(default)]
    pub workspace_roots: Vec<String>,
    #[serde(default)]
    pub last_session_start: Option<u64>,
    #[serde(default)]
    pub user_intent_summary: Option<String>,
    #[serde(default)]
    pub verification_command: Option<String>,
    #[serde(default)]
    pub expected_output_keyword: Option<String>,
    #[serde(default)]
    pub verification_passed: bool,
    #[serde(default)]
    pub last_risk_level: Option<String>,
    #[serde(default)]
    pub active_phase: Option<String>,
    #[serde(default)]
    pub active_task: Option<String>,
    #[serde(default)]
    pub recent_tasks: Vec<String>,
    #[serde(default)]
    pub edit_authorized: bool,
    #[serde(default)]
    pub active_architecture_pattern: Option<String>,
    #[serde(default)]
    pub modified_files: Vec<String>,
    #[serde(default)]
    pub dirty_files: Vec<String>,
    #[serde(default)]
    pub deferred_authorized_paths: Vec<String>,
    #[serde(default)]
    pub is_fast_path: bool,
    #[serde(default)]
    pub fast_path_files: Vec<String>,
    #[serde(skip, default)]
    pub pending_skill_proposals: Vec<(String, String, f32)>,
    #[serde(skip, default)]
    pub cancellation: Option<Arc<AtomicBool>>,
    #[serde(skip, default)]
    pub ast_cache: crate::mcp::tools::context_cache::AstResponseCache,
}

fn default_workflow_stage() -> String {
    "Context".to_string()
}

pub fn generate_session_id() -> String {
    let pid = std::process::id();
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let seq = SESSION_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("session_{}_{}_{}", pid, ts, seq)
}

impl Default for ServerState {
    fn default() -> Self {
        Self {
            session_id: generate_session_id(),
            priority_gate_passed: false,
            workflow_stage: "Context".to_string(),
            plan_approved: false,
            fix_attempts: 0,
            tool_calls: 0,
            tokens_original: 0,
            tokens_optimized: 0,
            last_raw_baseline_tokens: None,
            project_path: None,
            agent_client_name: None,
            workspace_roots: Vec::new(),
            last_session_start: None,
            user_intent_summary: None,
            verification_command: None,
            expected_output_keyword: None,
            verification_passed: false,
            last_risk_level: None,
            active_phase: None,
            active_task: None,
            recent_tasks: Vec::new(),
            edit_authorized: false,
            active_architecture_pattern: None,
            modified_files: Vec::new(),
            dirty_files: Vec::new(),
            deferred_authorized_paths: Vec::new(),
            is_fast_path: false,
            fast_path_files: Vec::new(),
            pending_skill_proposals: Vec::new(),
            cancellation: None,
            ast_cache: crate::mcp::tools::context_cache::AstResponseCache::default(),
        }
    }
}

fn sanitize_client_id_segment(name: &str) -> String {
    let mut out = String::with_capacity(name.len().min(40));
    let mut last_was_sep = false;
    for ch in name.trim().chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            last_was_sep = false;
            Some(ch.to_ascii_lowercase())
        } else if ch == '-' || ch == '_' || ch.is_whitespace() {
            if last_was_sep || out.is_empty() {
                None
            } else {
                last_was_sep = true;
                Some('_')
            }
        } else if !last_was_sep && !out.is_empty() {
            last_was_sep = true;
            Some('_')
        } else {
            None
        };
        if let Some(c) = mapped {
            out.push(c);
            if out.len() >= 40 {
                break;
            }
        }
    }
    let trimmed = out.trim_matches('_').to_string();
    if trimmed.is_empty() {
        "client".to_string()
    } else {
        trimmed
    }
}

fn canonicalize_or_normalize(path: &Path) -> std::path::PathBuf {
    let resolved = if let Ok(c) = path.canonicalize() {
        c
    } else {
        let mut suffix_parts: Vec<std::ffi::OsString> = Vec::new();
        let mut cur = path;
        let base = loop {
            if let Ok(c) = cur.canonicalize() {
                break Some(c);
            }
            if let Some(name) = cur.file_name() {
                suffix_parts.push(name.to_os_string());
                match cur.parent() {
                    Some(p) if !p.as_os_str().is_empty() => cur = p,
                    _ => break None,
                }
            } else {
                break None;
            }
        };
        let combined = match base {
            Some(mut b) => {
                for part in suffix_parts.into_iter().rev() {
                    b.push(part);
                }
                b
            }
            None => path.to_path_buf(),
        };
        lexical_normalize(&combined)
    };
    strip_verbatim_prefix(&resolved)
}

fn lexical_normalize(path: &Path) -> std::path::PathBuf {
    use std::path::Component;
    let mut out = std::path::PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push(comp.as_os_str());
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn strip_verbatim_prefix(path: &Path) -> std::path::PathBuf {
    if cfg!(windows) {
        let s = path.to_string_lossy();
        if let Some(stripped) = s.strip_prefix(r"\\?\") {
            return std::path::PathBuf::from(stripped);
        }
    }
    path.to_path_buf()
}

impl ServerState {
    pub fn new() -> Self {
        Self::with_client_name(None)
    }

    pub fn with_client_name(client_name: Option<String>) -> Self {
        let mut s = Self::default();
        if let Some(ref name) = client_name {
            let ts = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let seq = SESSION_SEQ.fetch_add(1, Ordering::Relaxed);
            let safe_name = sanitize_client_id_segment(name);
            s.session_id = format!(
                "session_{}_{}_{}_{}",
                std::process::id(),
                safe_name,
                ts,
                seq
            );
        }
        s.agent_client_name = client_name;
        s
    }

    pub fn set_roots_from_initialize(&mut self, params: &Value) {
        let mut parsed_roots = Vec::new();

        if let Some(roots) = params.get("roots").and_then(|r| r.as_array()) {
            for r in roots {
                if let Some(uri) = r.get("uri").and_then(|u| u.as_str()) {
                    let path_str = parse_file_uri(uri);
                    if !path_str.is_empty() {
                        parsed_roots.push(path_str);
                    }
                }
            }
        }

        if let Some(folders) = params.get("workspaceFolders").and_then(|f| f.as_array()) {
            for f in folders {
                if let Some(uri) = f.get("uri").and_then(|u| u.as_str()) {
                    let path_str = parse_file_uri(uri);
                    if !path_str.is_empty() && !parsed_roots.contains(&path_str) {
                        parsed_roots.push(path_str);
                    }
                }
            }
        }

        if let Some(root_uri) = params.get("rootUri").and_then(|u| u.as_str()) {
            let path_str = parse_file_uri(root_uri);
            if !path_str.is_empty() && !parsed_roots.contains(&path_str) {
                parsed_roots.push(path_str);
            }
        }

        if let Some(root_path) = params.get("rootPath").and_then(|p| p.as_str()) {
            let path_str = root_path.trim().to_string();
            if !path_str.is_empty() && !parsed_roots.contains(&path_str) {
                parsed_roots.push(path_str);
            }
        }

        if !parsed_roots.is_empty() {
            info!("Captured workspace roots from initialize: {:?}", parsed_roots);
            self.workspace_roots = parsed_roots.clone();

            // Background pre-warm workspace snapshot and skills before user sends first action
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                for root in parsed_roots {
                    let r_path = std::path::PathBuf::from(root);
                    handle.spawn_blocking(move || {
                        let _ = crate::context::cache::project_snapshot(&r_path);
                    });
                }
            }
        }
    }

    pub fn queue_deferred_authorization(&mut self, paths: &[String]) {
        for p in paths {
            if !self.deferred_authorized_paths.contains(p) {
                self.deferred_authorized_paths.push(p.clone());
            }
        }
    }

    pub fn drain_deferred_authorized_paths(&mut self) -> Vec<String> {
        std::mem::take(&mut self.deferred_authorized_paths)
    }

    pub fn clear_deferred_authorized_paths(&mut self) {
        self.deferred_authorized_paths.clear();
    }

    pub fn record_fast_path_file(&mut self, file: &str) {
        let clean = file
            .trim()
            .trim_start_matches("./")
            .trim_start_matches(".\\")
            .trim_start_matches(['/', '\\'])
            .replace('\\', "/");
        if !clean.is_empty() && !self.fast_path_files.iter().any(|f| f == &clean) {
            self.fast_path_files.push(clean);
        }
    }

    pub fn clear_fast_path(&mut self) {
        self.is_fast_path = false;
        self.fast_path_files.clear();
    }

    /// Records a relative file path modified during this active session.
    pub fn record_modified_file(&mut self, rel_path: &str) {
        let clean = rel_path
            .trim()
            .trim_start_matches(|c| c == '/' || c == '\\')
            .replace('\\', "/");
        if !clean.is_empty() {
            self.ast_cache.invalidate_file(&clean);
            if !self.modified_files.contains(&clean) {
                self.modified_files.push(clean.clone());
            }
            if !self.dirty_files.contains(&clean) {
                self.dirty_files.push(clean);
            }
        }
    }

    /// Records a task to the rolling recent tasks list (max 5).
    pub fn push_recent_task(&mut self, task: &str) {
        let t = task.trim();
        if t.is_empty() || t == "general task" {
            return;
        }
        if self.recent_tasks.last().map(|s| s.as_str()) == Some(t) {
            return;
        }
        self.recent_tasks.push(t.to_string());
        if self.recent_tasks.len() > 5 {
            self.recent_tasks.remove(0);
        }
    }

    /// Drains all dirty files queued for write-through invalidation.
    pub fn drain_dirty_files(&mut self) -> Vec<String> {
        let dirty = std::mem::take(&mut self.dirty_files);
        for f in &dirty {
            self.ast_cache.invalidate_file(f);
        }
        dirty
    }
    pub fn record_call(&mut self, orig_tokens: u64, opt_tokens: u64) {
        self.tool_calls += 1;
        self.tokens_original += orig_tokens;
        self.tokens_optimized += opt_tokens;
    }

    pub fn set_cancellation(&mut self, cancellation: Arc<AtomicBool>) {
        self.cancellation = Some(cancellation);
    }

    pub fn clear_cancellation(&mut self) {
        self.cancellation = None;
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancellation
            .as_ref()
            .map(|flag| flag.load(Ordering::Relaxed))
            .unwrap_or(false)
    }

    pub fn is_path_inside_workspace(&self, target: &Path) -> bool {
        let clean = canonicalize_or_normalize(target);
        self.workspace_roots
            .iter()
            .any(|r| clean.starts_with(canonicalize_or_normalize(Path::new(r))))
            || self
                .project_path
                .as_deref()
                .is_some_and(|p| clean.starts_with(canonicalize_or_normalize(Path::new(p))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_with_client_name_sanitizes_session_id() {
        let s = ServerState::with_client_name(Some("Claude Desktop (v1.2) / ../evil".to_string()));
        assert!(
            ServerState::is_valid_session_id(&s.session_id),
            "Sanitized session_id must be valid: {}",
            s.session_id
        );
    }

    #[test]
    fn test_set_roots_from_initialize_safe_without_tokio_runtime() {
        let mut state = ServerState::new();
        let dir = std::env::temp_dir().join(format!(
            "ag_init_notokio_{}_{}",
            std::process::id(),
            SESSION_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::create_dir_all(&dir);
        state.set_roots_from_initialize(&serde_json::json!({
            "rootPath": dir.to_string_lossy()
        }));
        assert_eq!(state.workspace_roots.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_is_path_inside_workspace_handles_nonexistent_and_blocks_traversal() {
        let base = std::env::temp_dir().join(format!(
            "ag_ws_inside_{}_{}",
            std::process::id(),
            SESSION_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let repo = base.join("repo");
        let _ = std::fs::create_dir_all(&repo);

        let mut state = ServerState::new();
        state.workspace_roots = vec![repo.to_string_lossy().to_string()];

        let nonexistent_child = repo.join("new_subdir").join("file.rs");
        assert!(
            state.is_path_inside_workspace(&nonexistent_child),
            "Non-existent child inside workspace must be recognized"
        );

        let traversal = repo.join("..").join("..").join("outside_evil.rs");
        assert!(
            !state.is_path_inside_workspace(&traversal),
            "Traversal outside workspace must be rejected"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn test_record_fast_path_file_normalizes_slashes() {
        let mut state = ServerState::new();
        state.record_fast_path_file("src\\main.rs");
        state.record_fast_path_file("./src/main.rs");
        state.record_fast_path_file("/src/main.rs");
        assert_eq!(state.fast_path_files, vec!["src/main.rs"]);
    }
}
