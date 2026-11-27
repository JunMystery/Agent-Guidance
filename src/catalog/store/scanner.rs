use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use super::{SkillItem, SkillSource};

const MAX_SCAN_DEPTH: usize = 12;

pub fn scan_workspace_skills(proj_path: &Path) -> Vec<SkillItem> {
    let mut results = Vec::new();
    #[allow(unused_mut)]
    let mut search_dirs = vec![
        proj_path.join(".agents").join("skills"),
        proj_path.join(".opencode").join("skills"),
        proj_path.join(".claude").join("skills"),
    ];

    #[cfg(not(test))]
    if let Some(home) = dirs::home_dir() {
        search_dirs.push(home.join(".agents").join("skills"));
        search_dirs.push(home.join(".agent-guidance").join("skills"));
    }

    for base_dir in search_dirs {
        if base_dir.exists() && base_dir.is_dir() {
            scan_skill_dir_recursive(&base_dir, &base_dir, &mut results);
        }
    }

    results
}

fn is_ignored_dir(dir_name: &str) -> bool {
    let lower = dir_name.to_lowercase();
    matches!(
        lower.as_str(),
        ".git"
            | ".github"
            | "node_modules"
            | "target"
            | "assets"
            | "img"
            | "images"
            | "dist"
            | "build"
            | "references"
            | "scripts"
            | "__pycache__"
            | "vendor"
            | "docs"
    )
}

fn is_ignored_doc_file(filename: &str) -> bool {
    let lower = filename.to_lowercase();
    matches!(
        lower.as_str(),
        "readme.md"
            | "changelog.md"
            | "contributing.md"
            | "license.md"
            | "index.md"
            | "template.md"
            | "_template.md"
            | "security.md"
            | "code_of_conduct.md"
            | "authors.md"
    )
}

pub(crate) fn scan_skill_dir_recursive(root_dir: &Path, current_dir: &Path, results: &mut Vec<SkillItem>) {
    let mut visited = HashSet::new();
    scan_skill_dir_inner(root_dir, current_dir, 0, &mut visited, results);
}

fn scan_skill_dir_inner(
    root_dir: &Path,
    current_dir: &Path,
    depth: usize,
    visited: &mut HashSet<PathBuf>,
    results: &mut Vec<SkillItem>,
) {
    if depth > MAX_SCAN_DEPTH {
        return;
    }

    let canonical = fs::canonicalize(current_dir).unwrap_or_else(|_| current_dir.to_path_buf());
    if !visited.insert(canonical) {
        return;
    }

    let dir_name = current_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();

    if depth > 0 && is_ignored_dir(&dir_name) {
        return;
    }

    // 1. Standard SKILL.md check
    let skill_md = current_dir.join("SKILL.md");
    if skill_md.is_file() {
        if let Ok(content) = fs::read_to_string(&skill_md) {
            let fallback_name = current_dir
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("custom-skill")
                .to_string();
            let name = extract_frontmatter_name(&content).unwrap_or(fallback_name);
            let rel_path = skill_md
                .strip_prefix(root_dir)
                .unwrap_or(&skill_md)
                .to_string_lossy()
                .to_string();
            results.push(SkillItem {
                name,
                relative_path: rel_path,
                source: SkillSource::LocalWorkspace(skill_md.to_string_lossy().to_string()),
                content,
            });
        }
        return;
    }

    // 2. OWASP / Cheatsheets directory check
    if dir_name == "cheatsheets" {
        if let Ok(entries) = fs::read_dir(current_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let is_md = path
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|e| e.eq_ignore_ascii_case("md"))
                    .unwrap_or(false);
                if path.is_file() && is_md {
                    let filename = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                    if is_ignored_doc_file(filename) {
                        continue;
                    }
                    if let Ok(content) = fs::read_to_string(&path) {
                        let fallback_name = path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("cheatsheet")
                            .replace('_', "-")
                            .to_lowercase();
                        let name = extract_skill_name(&content, &fallback_name);
                        let rel_path = path
                            .strip_prefix(root_dir)
                            .unwrap_or(&path)
                            .to_string_lossy()
                            .to_string();
                        results.push(SkillItem {
                            name,
                            relative_path: rel_path,
                            source: SkillSource::LocalWorkspace(path.to_string_lossy().to_string()),
                            content,
                        });
                    }
                }
            }
        }
        return;
    }

    // 3. Recurse into subdirectories
    if let Ok(entries) = fs::read_dir(current_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                scan_skill_dir_inner(root_dir, &path, depth + 1, visited, results);
            }
        }
    }
}

pub(crate) fn extract_frontmatter_name(content: &str) -> Option<String> {
    let clean = content.strip_prefix('\u{feff}').unwrap_or(content);
    let mut lines = clean.lines();
    let first = lines.find(|l| !l.trim().is_empty())?;
    if first.trim() != "---" {
        return None;
    }
    let mut found_name: Option<String> = None;
    for line in lines {
        let trimmed = line.trim();
        if trimmed == "---" {
            return found_name;
        }
        if found_name.is_none() && !line.starts_with([' ', '\t']) {
            let Some(rest) = trimmed.strip_prefix("name:") else {
                continue;
            };
            let cleaned = rest.trim().trim_matches('"').trim_matches('\'').trim();
            if !cleaned.is_empty() {
                found_name = Some(cleaned.to_string());
            }
        }
    }
    None
}

pub(crate) fn extract_skill_name(content: &str, fallback: &str) -> String {
    if let Some(fm_name) = extract_frontmatter_name(content) {
        return fm_name;
    }
    let mut open_fence: Option<(char, usize)> = None;
    for line in content.lines() {
        if crate::optimizer::compressor::update_code_fence(line, &mut open_fence) || open_fence.is_some() {
            continue;
        }
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("# ") {
            let title = rest.trim();
            if !title.is_empty() {
                let slug: String = title
                    .to_lowercase()
                    .replace(' ', "-")
                    .replace('_', "-")
                    .chars()
                    .filter(|c| c.is_alphanumeric() || *c == '-')
                    .collect();
                if !slug.is_empty() {
                    return slug;
                }
            }
        }
    }
    fallback.to_string()
}
