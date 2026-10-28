use std::path::{Path, PathBuf};
use crate::mcp::state::ServerState;

pub fn is_generic_home_dir(p: &Path) -> bool {
    if p.parent().is_none() || p == Path::new("/") || p == Path::new("") || p.components().count() <= 1 {
        return true;
    }
    let p_lossy = p.to_string_lossy();
    if p_lossy.len() <= 3 && p_lossy.contains(':') {
        return true;
    }
    let clean_p = p_lossy.trim_end_matches(['/', '\\']);
    if let Some(home) = dirs::home_dir() {
        if clean_p.eq_ignore_ascii_case(home.to_string_lossy().trim_end_matches(['/', '\\'])) {
            return true;
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if clean_p.eq_ignore_ascii_case(home.trim_end_matches(['/', '\\'])) {
            return true;
        }
    }
    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        if clean_p.eq_ignore_ascii_case(userprofile.trim_end_matches(['/', '\\'])) {
            return true;
        }
    }
    false
}

fn is_ide_install_dir(p: &Path) -> bool {
    let s = p.to_string_lossy().to_lowercase();
    s.contains("appdata\\local\\programs")
        || s.contains("program files")
        || s.contains("/applications/")
        || s.contains("/usr/bin")
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
                    if cwd.is_dir() && !is_ide_install_dir(cwd) && !is_generic_home_dir(cwd) {
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
    let trimmed = explicit_path.trim();
    let is_dot = trimmed.is_empty() || trimmed == "." || trimmed == "./" || trimmed == ".\\";

    // 1. Explicit path passed directly by user/tool
    if !is_dot {
        let p = if !state.workspace_roots.is_empty() && !Path::new(trimmed).is_absolute() {
            PathBuf::from(&state.workspace_roots[0]).join(trimmed)
        } else {
            PathBuf::from(trimmed)
        };
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

pub fn check_cross_workspace_guard(
    proj_path: &Path,
    paths: &[String],
    state: &ServerState,
    allow_cross_workspace: bool,
) -> Option<String> {
    if allow_cross_workspace {
        return None;
    }
    if let Some(session_root_str) = state.workspace_roots.first() {
        let session_root = Path::new(session_root_str);
        let proj_canonical = proj_path.canonicalize().unwrap_or_else(|_| proj_path.to_path_buf());
        let sess_canonical = session_root.canonicalize().unwrap_or_else(|_| session_root.to_path_buf());

        let mut cross_target: Option<String> = None;
        if !proj_canonical.starts_with(&sess_canonical) {
            cross_target = Some(proj_path.display().to_string());
        } else {
            for p_str in paths {
                let p = Path::new(p_str);
                if p.is_absolute() {
                    let can_p = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
                    if !can_p.starts_with(&sess_canonical) {
                        cross_target = Some(p_str.clone());
                        break;
                    }
                }
            }
        }

        if let Some(target_loc) = cross_target {
            return Some(format!(
                "# Edit Approval Gate Authorization\n\n- Status: BLOCKED (CROSS_WORKSPACE_WARNING)\n- Active Workspace: `{}`\n- Target Location: `{}`\n- Target Files: `{}`\n\n> [!WARNING] **Cross-Workspace Modification Detected**:\n> Target file is outside active workspace (`{}`).\n> To authorize editing in an external workspace, re-run with `allow_cross_workspace=true`.",
                session_root.display(),
                target_loc,
                paths.join(", "),
                session_root.display()
            ));
        }
    }
    None
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
