use serde_json::Value;
use std::path::{Component, Path, PathBuf};
use tracing::info;

use super::ServerState;
use super::uri::parse_file_uri;

/// Strips the Windows `\\?\` verbatim prefix if present.
pub fn strip_verbatim_prefix(path: &Path) -> PathBuf {
    if cfg!(windows) {
        let s = path.to_string_lossy();
        if let Some(stripped) = s.strip_prefix(r"\\?\") {
            return PathBuf::from(stripped);
        }
    }
    path.to_path_buf()
}

/// Lexically normalizes `.` and `..` components without touching the filesystem.
pub fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
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

/// Canonicalizes `path` if it exists, or canonicalizes its deepest existing ancestor
/// and re-attaches the remaining suffix, stripping any Windows `\\?\` prefix.
pub fn canonicalize_or_normalize(path: &Path) -> PathBuf {
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

impl ServerState {
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
                    let r_path = PathBuf::from(root);
                    handle.spawn_blocking(move || {
                        let _ = crate::context::cache::project_snapshot(&r_path);
                    });
                }
            }
        }
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
    use std::sync::atomic::{AtomicU64, Ordering};

    static WS_TEST_SEQ: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn test_is_path_inside_workspace_handles_nonexistent_and_blocks_traversal() {
        let base = std::env::temp_dir().join(format!(
            "ag_ws_inside_{}_{}",
            std::process::id(),
            WS_TEST_SEQ.fetch_add(1, Ordering::Relaxed)
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
}
