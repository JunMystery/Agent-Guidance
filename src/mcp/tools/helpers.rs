use std::path::Path;
use crate::context::db::CodeGraphDb;
use crate::context::scanner::scan_project;
use crate::context::watcher::{is_watching, start_watching};
use crate::mcp::state::ServerState;

#[allow(unused_imports)]
pub use super::helpers_path::{
    detect_parent_process_cwd, detect_project_path, is_generic_home_dir, validate_path,
};

pub fn ensure_not_cancelled(state: &ServerState) -> Result<(), (i32, String)> {
    if state.is_cancelled() {
        Err((-32000, "Request cancelled after timeout".to_string()))
    } else {
        Ok(())
    }
}

pub fn detect_project_architecture(proj_path: &Path) -> String {
    // 1. Check persistent project architecture configuration if present
    if let Some(persisted) = ServerState::load_persisted_architecture(proj_path) {
        return persisted;
    }

    // 2. Check GraphRAG communities hierarchy if available
    if let Some(hierarchy) = crate::context::graph_rag::persistence::load_hierarchy(proj_path) {
        if !hierarchy.detected_architecture.is_empty()
            && !hierarchy.detected_architecture.eq_ignore_ascii_case("auto")
            && !hierarchy.detected_architecture.eq_ignore_ascii_case("none")
        {
            let _ = ServerState::save_persisted_architecture(proj_path, &hierarchy.detected_architecture);
            return hierarchy.detected_architecture;
        }
    }

    let files = scan_project(proj_path, 8);
    let paths: Vec<String> = files.into_iter().map(|f| f.path.to_lowercase()).collect();

    let detected = if paths.iter().any(|p| {
        p.contains("domain")
            || p.contains("usecase")
            || p.contains("use_case")
            || p.contains("infrastructure")
            || p.contains("infra")
            || p.contains("entities")
            || p.contains("entity")
    }) {
        "Clean_Architecture".to_string()
    } else if paths.iter().any(|p| {
        p.contains("controller")
            || p.contains("service")
            || p.contains("model")
            || p.contains("viewmodel")
            || p.contains("repository")
            || p.contains("dao")
            || p.contains("database")
    }) {
        "Layered_Architecture".to_string()
    } else if paths.iter().any(|p| {
        p.contains("feature")
            || p.contains("module")
            || p.contains("screens")
            || p.contains("pages")
    }) {
        "Package_By_Feature".to_string()
    } else if paths.iter().any(|p| {
        p.contains("commands")
            || p.contains("command")
            || p.contains("cli")
            || p.contains("cmd")
            || p.contains("args")
            || p.contains("opt")
    }) {
        "CLI_Pipeline".to_string()
    } else if paths.len() <= 12 {
        "Flat_Library".to_string()
    } else {
        "Orchestrator".to_string()
    };

    // Automatically persist the detected pattern to .agent-context/architecture.json
    let _ = ServerState::save_persisted_architecture(proj_path, &detected);
    detected
}

pub fn resolve_architecture_pattern(raw_pattern: &str, proj_path: &Path, state: &ServerState) -> String {
    let trimmed = raw_pattern.trim();
    if trimmed.is_empty()
        || trimmed.eq_ignore_ascii_case("auto")
        || trimmed.eq_ignore_ascii_case("none")
    {
        if let Some(active) = &state.active_architecture_pattern {
            if !active.eq_ignore_ascii_case("auto")
                && !active.eq_ignore_ascii_case("none")
                && !active.is_empty()
            {
                return active.clone();
            }
        }
        detect_project_architecture(proj_path)
    } else {
        trimmed.to_string()
    }
}



pub fn ensure_indexed(proj_path: &Path) -> Option<CodeGraphDb> {
    let _ = crate::context::graph_rag::jit_sync::ensure_fresh_graph(proj_path, 15);
    if !is_watching(proj_path) {
        start_watching(proj_path);
    }
    let start = std::time::Instant::now();
    while crate::context::graph_rag::jit_sync::is_indexing(proj_path)
        && start.elapsed() < std::time::Duration::from_millis(1000)
    {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    CodeGraphDb::open_for_project(proj_path).ok()
}

pub fn embed_query(query: &str) -> Option<Vec<f32>> {
    crate::ml::embeddings::try_cached_model().and_then(|model| {
        model.embed_text(query, Some("query")).ok()
    })
}

/// Truncate string to at most `max_chars` UTF-8 characters safely without splitting multibyte characters.
pub fn truncate_chars(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

/// Truncate string to at most `max_bytes` without splitting UTF-8 char boundaries.
#[allow(dead_code)]
pub fn truncate_bytes_safe(s: &str, max_bytes: usize) -> &str {
    if max_bytes >= s.len() {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}
