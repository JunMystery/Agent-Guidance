use std::path::{Path, PathBuf};
use crate::mcp::state::ServerState;

pub fn is_generic_home_dir(p: &Path) -> bool {
    if let Some(home) = dirs::home_dir() {
        if p == home {
            return true;
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if p == Path::new(&home) {
            return true;
        }
    }
    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        if p == Path::new(&userprofile) {
            return true;
        }
    }
    false
}

pub fn detect_parent_process_cwd() -> Option<PathBuf> {
    use sysinfo::{Pid, System};
    let mut sys = System::new_all();
    sys.refresh_all();

    let my_pid = Pid::from_u32(std::process::id());
    if let Some(proc_) = sys.process(my_pid) {
        if let Some(parent_pid) = proc_.parent() {
            if let Some(parent_proc) = sys.process(parent_pid) {
                if let Some(cwd) = parent_proc.cwd() {
                    if cwd.is_dir() && !cwd.to_string_lossy().to_lowercase().contains("antigravity") {
                        return Some(cwd.to_path_buf());
                    }
                }
            }
        }
    }
    None
}

/// Detects the active project path with strict session workspace isolation.
pub fn detect_project_path(explicit_path: &str, state: &ServerState) -> PathBuf {
    // 1. Explicit path passed directly by user/tool
    if explicit_path != "." && !explicit_path.trim().is_empty() {
        let p = PathBuf::from(explicit_path);
        if !is_generic_home_dir(&p) {
            return crate::dashboard::projects_path::find_project_root(&p);
        }
    }

    // 2. Strict Session Workspace Root: Bound directly by IDE initialize handshake
    if let Some(first_root) = state.workspace_roots.first() {
        let p = PathBuf::from(first_root);
        if !is_generic_home_dir(&p) {
            return crate::dashboard::projects_path::find_project_root(&p);
        }
    }

    // 3. Process CWD (the exact working directory the IDE process spawned this MCP server in)
    if let Ok(cwd) = std::env::current_dir() {
        if !is_generic_home_dir(&cwd) {
            return crate::dashboard::projects_path::find_project_root(&cwd);
        }
    }

    // 4. Parent process CWD
    if let Some(parent_cwd) = detect_parent_process_cwd() {
        if !is_generic_home_dir(&parent_cwd) {
            return crate::dashboard::projects_path::find_project_root(&parent_cwd);
        }
    }

    // 5. Explicit state project path previously established in this session
    if let Some(ref sp) = state.project_path {
        let p = PathBuf::from(sp);
        if !is_generic_home_dir(&p) {
            return crate::dashboard::projects_path::find_project_root(&p);
        }
    }

    // 6. Global persistent fallback ONLY if running headless CLI without IDE workspace
    if state.workspace_roots.is_empty() {
        if let Some(gp) = ServerState::read_global_project_path() {
            let p = PathBuf::from(&gp);
            if !is_generic_home_dir(&p) {
                return crate::dashboard::projects_path::find_project_root(&p);
            }
        }
    }

    PathBuf::from(".")
}

#[allow(dead_code)]
pub fn is_path_inside_workspace(base_path: &Path, target_path: &Path) -> bool {
    let clean_base = match base_path.canonicalize() {
        Ok(c) => c,
        Err(_) => base_path.to_path_buf(),
    };
    let clean_target = match target_path.canonicalize() {
        Ok(c) => c,
        Err(_) => target_path.to_path_buf(),
    };
    clean_target.starts_with(&clean_base)
}

/// Validate that a relative or absolute target stays within base directory root
pub fn validate_path(base_path: &Path, rel_path: &str) -> Result<PathBuf, String> {
    validate_path_scoped(base_path, rel_path, false)
}

pub fn validate_path_scoped(base_path: &Path, rel_path: &str, allow_cross_workspace: bool) -> Result<PathBuf, String> {
    if rel_path.contains("..") {
        return Err("Path traversal (..) is strictly prohibited.".to_string());
    }

    let p_rel = Path::new(rel_path);
    if allow_cross_workspace && p_rel.is_absolute() {
        if let Ok(can) = p_rel.canonicalize() {
            return Ok(can);
        }
        return Ok(p_rel.to_path_buf());
    }

    let canonical_base = base_path
        .canonicalize()
        .map_err(|e| format!("Invalid project path: {}", e))?;

    let clean_base = canonical_base.to_string_lossy();
    let clean_base_str = clean_base.strip_prefix(r"\\?\").unwrap_or(&clean_base);
    let p_clean_base = Path::new(clean_base_str);

    let stripped = if p_rel.is_absolute() {
        if let Ok(sub) = p_rel.strip_prefix(base_path) {
            sub.to_string_lossy().to_string()
        } else if let Ok(sub) = p_rel.strip_prefix(&canonical_base) {
            sub.to_string_lossy().to_string()
        } else if let Ok(sub) = p_rel.strip_prefix(p_clean_base) {
            sub.to_string_lossy().to_string()
        } else if let Ok(can_rel) = p_rel.canonicalize() {
            if let Ok(sub) = can_rel.strip_prefix(&canonical_base) {
                sub.to_string_lossy().to_string()
            } else {
                let candidate = rel_path.trim_start_matches(|c| c == '/' || c == '\\');
                let joined = canonical_base.join(candidate);
                if joined.exists() && joined.canonicalize().map(|p| p.starts_with(&canonical_base)).unwrap_or(false) {
                    candidate.to_string()
                } else {
                    return Err("Target path resolves outside workspace root.".to_string());
                }
            }
        } else {
            let candidate = rel_path.trim_start_matches(|c| c == '/' || c == '\\');
            let joined = canonical_base.join(candidate);
            if joined.starts_with(&canonical_base) {
                candidate.to_string()
            } else {
                return Err("Target path resolves outside workspace root.".to_string());
            }
        }
    } else {
        rel_path.to_string()
    };

    let clean_rel = stripped
        .trim()
        .trim_start_matches(|c| c == '/' || c == '\\')
        .replace('\\', "/");

    if clean_rel.is_empty() {
        return Ok(canonical_base);
    }

    let full_path = canonical_base.join(&clean_rel);

    if full_path.exists() {
        let canonical_full = full_path
            .canonicalize()
            .map_err(|e| format!("Invalid target path: {}", e))?;

        if !canonical_full.starts_with(&canonical_base) {
            return Err("Target path resolves outside workspace root.".to_string());
        }
        Ok(canonical_full)
    } else if full_path.starts_with(&canonical_base) {
        Ok(full_path)
    } else {
        Err("Target path resolves outside workspace root.".to_string())
    }
}
