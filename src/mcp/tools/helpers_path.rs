use std::path::{Component, Path, PathBuf};
use crate::mcp::state::ServerState;
use crate::mcp::state::workspace::{canonicalize_or_normalize, strip_verbatim_prefix};

pub use crate::dashboard::projects_container::{is_generic_container_dir, is_generic_home_dir};

fn is_ide_install_dir(p: &Path) -> bool {
    let s = p.to_string_lossy().to_lowercase();
    s.contains("appdata\\local\\programs")
        || s.contains("program files")
        || s.contains("/applications/")
        || s.contains("/usr/bin")
}

fn resolve_safe_root(p: &Path) -> PathBuf {
    let resolved = crate::dashboard::projects_path::find_project_root(p);
    if resolved != p && !resolved.join(".git").exists() {
        return p.to_path_buf();
    }
    if is_generic_container_dir(&resolved) {
        return p.to_path_buf();
    }
    resolved
}

pub fn detect_parent_process_cwd() -> Option<PathBuf> {
    use sysinfo::{Pid, System};
    let sys = System::new_all();

    let my_pid = Pid::from_u32(std::process::id());
    if let Some(proc_) = sys.process(my_pid)
        && let Some(parent_pid) = proc_.parent()
        && let Some(parent_proc) = sys.process(parent_pid)
        && let Some(cwd) = parent_proc.cwd()
        && cwd.is_dir()
        && !is_ide_install_dir(cwd)
        && !is_generic_container_dir(cwd)
    {
        return Some(cwd.to_path_buf());
    }
    None
}

/// Detects the active project path with strict session workspace isolation.
pub fn detect_project_path(explicit_path: &str, state: &ServerState) -> PathBuf {
    detect_project_path_with_targets(explicit_path, &[], state)
}

/// Detects the active project path with target path inference and session workspace isolation.
pub fn detect_project_path_with_targets(
    explicit_path: &str,
    target_paths: &[String],
    state: &ServerState,
) -> PathBuf {
    let trimmed = explicit_path.trim();
    let is_dot = trimmed.is_empty() || trimmed == "." || trimmed == "./" || trimmed == ".\\";

    // 1. Explicit path passed directly by user/tool
    if !is_dot {
        let p = if !state.workspace_roots.is_empty() && !Path::new(trimmed).is_absolute() {
            PathBuf::from(&state.workspace_roots[0]).join(trimmed)
        } else {
            PathBuf::from(trimmed)
        };
        if !is_generic_container_dir(&p) {
            return resolve_safe_root(&p);
        }
    }

    // 2. Target path inference: if target is absolute, resolve its project root
    for target in target_paths {
        let p_target = Path::new(target.trim());
        if p_target.is_absolute() {
            let start_dir = if p_target.is_dir() {
                p_target
            } else {
                p_target.parent().unwrap_or(p_target)
            };
            if !is_generic_container_dir(start_dir) {
                let inferred = resolve_safe_root(start_dir);
                if !is_generic_container_dir(&inferred)
                    && (state.workspace_roots.is_empty()
                        || state.workspace_roots.iter().any(|r| inferred.starts_with(r)))
                {
                    return inferred;
                }
            }
        }
    }

    // 3. Strict Session Workspace Root: Bound directly by IDE initialize handshake
    if let Some(first_root) = state.workspace_roots.first() {
        let p = PathBuf::from(first_root);
        if !is_generic_container_dir(&p) {
            return resolve_safe_root(&p);
        }
    }

    // 4. Explicit session project path previously established in this session (e.g. task_pipeline)
    if let Some(ref sp) = state.project_path {
        let p = PathBuf::from(sp);
        if !is_generic_container_dir(&p) {
            return resolve_safe_root(&p);
        }
    }

    // 5. Process CWD (fallback only if no session project path set)
    if let Ok(cwd) = std::env::current_dir()
        && !is_generic_container_dir(&cwd)
    {
        return resolve_safe_root(&cwd);
    }

    // 6. Parent process CWD
    if let Some(parent_cwd) = detect_parent_process_cwd()
        && !is_generic_container_dir(&parent_cwd)
    {
        return resolve_safe_root(&parent_cwd);
    }

    // 7. Global persistent fallback ONLY if running headless CLI without IDE workspace
    if state.workspace_roots.is_empty()
        && let Some(gp) = ServerState::read_global_project_path()
    {
        let p = PathBuf::from(&gp);
        if !is_generic_container_dir(&p) {
            return resolve_safe_root(&p);
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
    if allow_cross_workspace || state.workspace_roots.is_empty() {
        return None;
    }

    let canonical_roots: Vec<PathBuf> = state
        .workspace_roots
        .iter()
        .map(|r| canonicalize_or_normalize(Path::new(r)))
        .collect();
    let proj_canonical = canonicalize_or_normalize(proj_path);

    let mut cross_target: Option<String> = None;
    if !canonical_roots.iter().any(|root| proj_canonical.starts_with(root)) {
        cross_target = Some(proj_path.display().to_string());
    } else {
        for p_str in paths {
            let p = Path::new(p_str.trim());
            if p.is_absolute() {
                let can_p = canonicalize_or_normalize(p);
                if !canonical_roots.iter().any(|root| can_p.starts_with(root)) {
                    cross_target = Some(p_str.clone());
                    break;
                }
            }
        }
    }

    if let Some(target_loc) = cross_target {
        let roots_display = state.workspace_roots.join(", ");
        return Some(format!(
            "# Edit Approval Gate Authorization\n\n- Status: BLOCKED (CROSS_WORKSPACE_WARNING)\n- Active Workspace: `{}`\n- Target Location: `{}`\n- Target Files: `{}`\n\n> [!WARNING] **Cross-Workspace Modification Detected**:\n> Target file is outside active workspace (`{}`).\n> To authorize editing in an external workspace, re-run with `allow_cross_workspace=true`.",
            roots_display,
            target_loc,
            paths.join(", "),
            roots_display
        ));
    }

    None
}

fn has_parent_traversal(raw: &str) -> bool {
    Path::new(raw)
        .components()
        .any(|c| matches!(c, Component::ParentDir))
        || raw
            .replace('\\', "/")
            .split('/')
            .any(|seg| seg == "..")
}

fn normalize_posix_rel(raw: &str) -> String {
    raw.trim()
        .replace('\\', "/")
        .split('/')
        .filter(|seg| !seg.is_empty() && *seg != ".")
        .collect::<Vec<_>>()
        .join("/")
}

/// Validate that a relative or absolute target stays within base directory root.
pub fn validate_path(base_path: &Path, rel_path: &str) -> Result<PathBuf, String> {
    validate_path_scoped(base_path, rel_path, false)
}

fn strip_absolute_or_leading_prefix(
    base_path: &Path,
    canonical_base: &Path,
    p_clean_base: &Path,
    rel_path: &str,
) -> Result<String, String> {
    let p_rel = Path::new(rel_path);
    if !p_rel.is_absolute() {
        return Ok(rel_path.to_string());
    }
    if let Ok(sub) = p_rel
        .strip_prefix(base_path)
        .or_else(|_| p_rel.strip_prefix(canonical_base))
        .or_else(|_| p_rel.strip_prefix(p_clean_base))
    {
        return Ok(sub.to_string_lossy().to_string());
    }
    let normalized_abs = canonicalize_or_normalize(p_rel);
    if let Ok(sub) = normalized_abs.strip_prefix(p_clean_base) {
        return Ok(sub.to_string_lossy().to_string());
    }
    if let Ok(can_rel) = p_rel.canonicalize() {
        return can_rel
            .strip_prefix(canonical_base)
            .map(|sub| sub.to_string_lossy().to_string())
            .map_err(|_| "Target path resolves outside workspace root.".to_string());
    }
    // On Unix, virtual workspace-root paths like "/Cargo.toml" look absolute due to leading '/'.
    let candidate = normalize_posix_rel(rel_path);
    let has_drive = candidate.as_bytes().get(1) == Some(&b':');
    let joined = canonical_base.join(&candidate);
    if !has_drive
        && !candidate.is_empty()
        && joined.exists()
        && joined
            .canonicalize()
            .map(|p| p.starts_with(canonical_base))
            .unwrap_or(false)
    {
        return Ok(candidate);
    }
    Err("Target path resolves outside workspace root.".to_string())
}

fn resolve_nonexistent_within_base(
    canonical_base: &Path,
    full_path: &Path,
) -> Result<PathBuf, String> {
    let mut missing_parts: Vec<std::ffi::OsString> = Vec::new();
    let mut cur = full_path;
    let ancestor = loop {
        if cur.exists() {
            break cur
                .canonicalize()
                .map_err(|e| format!("Invalid target path ancestor: {}", e))?;
        }
        if let Some(name) = cur.file_name() {
            missing_parts.push(name.to_os_string());
            match cur.parent() {
                Some(p) if !p.as_os_str().is_empty() => cur = p,
                _ => return Err("Target path resolves outside workspace root.".to_string()),
            }
        } else {
            return Err("Target path resolves outside workspace root.".to_string());
        }
    };

    if !ancestor.starts_with(canonical_base) {
        return Err("Target path resolves outside workspace root.".to_string());
    }

    let mut resolved = ancestor;
    for part in missing_parts.into_iter().rev() {
        resolved.push(part);
    }
    Ok(resolved)
}

/// Validates a path against `base_path`, blocking null bytes, `..` traversal, and symlink escapes,
/// and returns `(resolved_full_path, normalized_posix_relative_path)`.
pub fn validate_path_scoped_normalized(
    base_path: &Path,
    rel_path: &str,
    allow_cross_workspace: bool,
) -> Result<(PathBuf, String), String> {
    if rel_path.contains('\0') {
        return Err("Null byte in path is strictly prohibited.".to_string());
    }
    if has_parent_traversal(rel_path) {
        return Err("Path traversal (..) is strictly prohibited.".to_string());
    }

    let p_rel = Path::new(rel_path);
    if allow_cross_workspace && p_rel.is_absolute() {
        let resolved = if p_rel.exists() {
            p_rel
                .canonicalize()
                .unwrap_or_else(|_| p_rel.to_path_buf())
        } else {
            canonicalize_or_normalize(p_rel)
        };
        let norm_rel = base_path
            .canonicalize()
            .ok()
            .and_then(|can_base| {
                let clean_base = strip_verbatim_prefix(&can_base);
                let clean_res = strip_verbatim_prefix(&resolved);
                clean_res
                    .strip_prefix(&clean_base)
                    .ok()
                    .map(|sub| normalize_posix_rel(&sub.to_string_lossy()))
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                strip_verbatim_prefix(&resolved)
                    .to_string_lossy()
                    .replace('\\', "/")
            });
        return Ok((resolved, norm_rel));
    }

    let canonical_base = base_path
        .canonicalize()
        .map_err(|e| format!("Invalid project path: {}", e))?;

    let p_clean_base = strip_verbatim_prefix(&canonical_base);

    let stripped =
        strip_absolute_or_leading_prefix(base_path, &canonical_base, &p_clean_base, rel_path)?;

    let clean_rel = normalize_posix_rel(&stripped);

    if clean_rel.is_empty() {
        return Ok((canonical_base, String::new()));
    }

    let full_path = canonical_base.join(&clean_rel);

    if full_path.exists() {
        let canonical_full = full_path
            .canonicalize()
            .map_err(|e| format!("Invalid target path: {}", e))?;

        if !canonical_full.starts_with(&canonical_base) {
            return Err("Target path resolves outside workspace root.".to_string());
        }
        Ok((canonical_full, clean_rel))
    } else {
        let resolved = resolve_nonexistent_within_base(&canonical_base, &full_path)?;
        Ok((resolved, clean_rel))
    }
}

pub fn validate_path_scoped(
    base_path: &Path,
    rel_path: &str,
    allow_cross_workspace: bool,
) -> Result<PathBuf, String> {
    validate_path_scoped_normalized(base_path, rel_path, allow_cross_workspace).map(|(p, _)| p)
}
