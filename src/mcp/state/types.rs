use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{path::Path, sync::Arc, sync::atomic::{AtomicBool, Ordering}, time::{SystemTime, UNIX_EPOCH}};
use tracing::info;

pub const SESSION_STALE_TIMEOUT_SECS: u64 = 300;

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
    format!("session_{}_{}", pid, ts)
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

impl ServerState {
    pub fn new() -> Self {
        Self::with_client_name(None)
    }

    pub fn with_client_name(client_name: Option<String>) -> Self {
        let mut s = Self::default();
        if let Some(ref name) = client_name {
            s.session_id = format!(
                "session_{}_{}",
                std::process::id(),
                name.to_lowercase().replace(' ', "_")
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
            for root in parsed_roots {
                let r_path = std::path::PathBuf::from(root);
                tokio::task::spawn_blocking(move || {
                    let _ = crate::context::cache::project_snapshot(&r_path);
                });
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
        if !self.fast_path_files.iter().any(|f| f == file) {
            self.fast_path_files.push(file.to_string());
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
        let clean = target.canonicalize().unwrap_or_else(|_| target.to_path_buf());
        self.workspace_roots.iter().any(|r| {
            clean.starts_with(Path::new(r).canonicalize().unwrap_or_else(|_| std::path::PathBuf::from(r)))
        }) || self.project_path.as_deref().map_or(false, |p| {
            clean.starts_with(Path::new(p).canonicalize().unwrap_or_else(|_| std::path::PathBuf::from(p)))
        })
    }
}
