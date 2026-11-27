use std::path::Path;

/// Checks if a path is the filesystem root, user home directory, or a drive root.
pub fn is_generic_home_dir(p: &Path) -> bool {
    if p.parent().is_none() || p == Path::new("/") || p == Path::new("") || p.components().count() <= 1 {
        return true;
    }
    let p_lossy = p.to_string_lossy();
    if p_lossy.len() <= 3 && p_lossy.contains(':') {
        return true;
    }
    let clean_p = p_lossy.trim_end_matches(['/', '\\']);
    let check_match = |var_val: Option<String>| {
        var_val.map(|v| clean_p.eq_ignore_ascii_case(v.trim_end_matches(['/', '\\']))).unwrap_or(false)
    };
    if check_match(dirs::home_dir().map(|h| h.to_string_lossy().to_string()))
        || check_match(std::env::var("HOME").ok())
        || check_match(std::env::var("USERPROFILE").ok())
    {
        return true;
    }

    // Recognize common user home parent patterns like /home/<user>, /Users/<user>, or C:\Users\<user>
    if let Some(parent) = p.parent() {
        if parent == Path::new("/home") || parent == Path::new("/Users") {
            return true;
        }
        let clean_parent = parent
            .to_string_lossy()
            .trim_end_matches(['/', '\\'])
            .to_ascii_lowercase();
        if clean_parent.ends_with(":\\users") || clean_parent.ends_with(":/users") {
            return true;
        }
    }
    false
}

/// Checks if a directory is a generic workspace/repo container (e.g. ~/Git, ~/Projects, ~/workspace)
/// that holds multiple independent projects and should NEVER be treated as a single project root.
pub fn is_generic_container_dir(p: &Path) -> bool {
    if is_generic_home_dir(p) {
        return true;
    }

    let p_lossy = p.to_string_lossy();
    let clean_p = p_lossy.trim_end_matches(['/', '\\']);
    let path = Path::new(clean_p);

    // 1. Directory named with common container patterns that does not have its own .git
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        let lower = name.to_lowercase();
        let is_unambiguous_container = matches!(
            lower.as_str(),
            "git"
                | "github"
                | "gitlab"
                | "projects"
                | "workspace"
                | "workspaces"
                | "repos"
                | "repositories"
        );
        let is_contextual_container = matches!(
            lower.as_str(),
            "code" | "dev" | "development" | "project"
        );

        if !path.join(".git").exists() {
            if is_unambiguous_container {
                return true;
            }
            if is_contextual_container
                && let Some(parent) = path.parent()
                && (is_generic_home_dir(parent)
                    || parent.parent().is_none()
                    || parent == Path::new("/")
                    || parent == Path::new("")
                    || parent.parent() == Some(Path::new("/"))
                    || parent.components().count() <= 3)
            {
                return true;
            }
        }
    }

    // 2. Heuristic: Directory does not have .git itself, but contains >= 2 subdirectories with .git
    if !path.join(".git").exists()
        && let Ok(entries) = std::fs::read_dir(path)
    {
        let mut git_repo_count = 0;
        for entry in entries.flatten().take(30) {
            if let Ok(ft) = entry.file_type()
                && ft.is_dir()
                && entry.path().join(".git").exists()
            {
                git_repo_count += 1;
                if git_repo_count >= 2 {
                    return true;
                }
            }
        }
    }

    false
}
