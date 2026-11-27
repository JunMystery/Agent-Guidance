use crate::context::scanner::FileEntry;
use std::collections::HashSet;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct ProjectLanguageProfile {
    pub primary_languages: HashSet<String>,
    pub secondary_tech: HashSet<String>,
}

/// Returns true if `needle` appears in `haystack` bounded on both sides by non-alphanumeric characters.
pub(crate) fn contains_word(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    for (idx, _) in haystack.match_indices(needle) {
        let before_ok = haystack[..idx]
            .chars()
            .next_back()
            .map(|c| !c.is_alphanumeric())
            .unwrap_or(true);
        let after_ok = haystack[idx + needle.len()..]
            .chars()
            .next()
            .map(|c| !c.is_alphanumeric())
            .unwrap_or(true);
        if before_ok && after_ok {
            return true;
        }
    }
    false
}

/// Returns true if `needle` appears in `haystack` starting at a non-alphanumeric boundary.
pub(crate) fn contains_word_prefix(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    for (idx, _) in haystack.match_indices(needle) {
        let before_ok = haystack[..idx]
            .chars()
            .next_back()
            .map(|c| !c.is_alphanumeric())
            .unwrap_or(true);
        if before_ok {
            return true;
        }
    }
    false
}

pub fn detect_language_profile(files: &[FileEntry], task_prompt: &str) -> ProjectLanguageProfile {
    let mut ext_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for file in files {
        if file.file_type == "dir" {
            continue;
        }
        let path = file.path.to_lowercase();
        if let Some(ext) = Path::new(&path).extension().and_then(|e| e.to_str()) {
            *ext_counts.entry(ext.to_string()).or_insert(0) += 1;
        }
        let file_name = Path::new(&path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        if file_name.contains("docker") || path.split(['/', '\\']).any(|seg| seg == "docker") {
            *ext_counts.entry("docker".to_string()).or_insert(0) += 1;
        }
    }

    let mut profile = ProjectLanguageProfile::default();

    // Map extensions to primary programming languages
    for (ext, _count) in &ext_counts {
        match ext.as_str() {
            "rs" => {
                profile.primary_languages.insert("rust".to_string());
            }
            "py" | "pyw" => {
                profile.primary_languages.insert("python".to_string());
            }
            "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => {
                profile.primary_languages.insert("javascript".to_string());
                profile.primary_languages.insert("typescript".to_string());
            }
            "go" => {
                profile.primary_languages.insert("go".to_string());
            }
            "java" => {
                profile.primary_languages.insert("java".to_string());
            }
            "cpp" | "cxx" | "cc" | "c" | "h" | "hpp" => {
                profile.primary_languages.insert("c++".to_string());
                profile.primary_languages.insert("c".to_string());
            }
            "rb" => {
                profile.primary_languages.insert("ruby".to_string());
            }
            "php" => {
                profile.primary_languages.insert("php".to_string());
            }
            "cs" => {
                profile.primary_languages.insert("c#".to_string());
            }
            "kt" | "kts" => {
                profile.primary_languages.insert("kotlin".to_string());
            }
            "swift" => {
                profile.primary_languages.insert("swift".to_string());
            }
            // Secondary tech & domain languages (even if 1-2 files exist)
            "sql" => {
                profile.secondary_tech.insert("sql".to_string());
                profile.secondary_tech.insert("database".to_string());
            }
            "html" | "css" | "scss" | "vue" | "svelte" => {
                profile.secondary_tech.insert("web".to_string());
                profile.secondary_tech.insert("frontend".to_string());
            }
            "docker" | "dockerfile" => {
                profile.secondary_tech.insert("docker".to_string());
                profile.secondary_tech.insert("devops".to_string());
            }
            "sh" | "bash" | "zsh" => {
                profile.secondary_tech.insert("bash".to_string());
                profile.secondary_tech.insert("shell".to_string());
            }
            _ => {}
        }
    }

    // Explicit User Mentions in prompt override/add to profile
    // Match mode: 0 = substring, 1 = word prefix, 2 = whole word
    let prompt_lower = task_prompt.to_lowercase();
    let known_tech: [(&str, &str, u8); 19] = [
        ("rust", "rust", 2),
        ("python", "python", 0),
        ("javascript", "javascript", 0),
        ("typescript", "typescript", 0),
        ("golang", "go", 0),
        ("go", "go", 2),
        ("java", "java", 2),
        ("kotlin", "kotlin", 2),
        ("android", "kotlin", 0),
        ("swift", "swift", 2),
        ("c++", "c++", 1),
        ("c#", "c#", 1),
        ("sql", "sql", 0),
        ("database", "database", 0),
        ("docker", "docker", 0),
        ("frontend", "frontend", 0),
        ("html", "web", 2),
        ("css", "web", 2),
        ("bash", "bash", 2),
    ];

    for (keyword, tech, mode) in known_tech {
        let matched = match mode {
            2 => contains_word(&prompt_lower, keyword),
            1 => contains_word_prefix(&prompt_lower, keyword),
            _ => prompt_lower.contains(keyword),
        };
        if matched {
            profile.primary_languages.insert(tech.to_string());
            profile.secondary_tech.insert(tech.to_string());
        }
    }

    profile
}

/// Fast, shallow language profile detection avoiding full depth-8 directory walk (< 2ms).
pub fn detect_language_fast(proj_path: &std::path::Path, task: &str) -> ProjectLanguageProfile {
    let mut files = Vec::new();
    let mut dirs_to_visit = vec![(proj_path.to_path_buf(), 0usize)];

    while let Some((dir, depth)) = dirs_to_visit.pop() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten().take(50) {
                let path = entry.path();
                let rel_str = path
                    .strip_prefix(proj_path)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string();
                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                let file_type = if is_dir { "dir".to_string() } else { "file".to_string() };
                files.push(FileEntry {
                    path: rel_str,
                    file_type,
                    size_bytes: 0,
                });
                if is_dir && depth < 3 {
                    let dir_name = entry.file_name().to_string_lossy().to_lowercase();
                    if !dir_name.starts_with('.') && dir_name != "target" && dir_name != "node_modules" && dir_name != "build" {
                        dirs_to_visit.push((path, depth + 1));
                    }
                }
            }
        }
        if files.len() >= 100 {
            break;
        }
    }

    let mut profile = detect_language_profile(&files, task);
    if proj_path.join("Cargo.toml").exists() {
        profile.primary_languages.insert("rust".to_string());
    }
    if proj_path.join("package.json").exists() {
        profile.primary_languages.insert("javascript".to_string());
        profile.primary_languages.insert("typescript".to_string());
    }
    if proj_path.join("go.mod").exists() {
        profile.primary_languages.insert("go".to_string());
    }
    if proj_path.join("pyproject.toml").exists() || proj_path.join("requirements.txt").exists() {
        profile.primary_languages.insert("python".to_string());
    }
    if proj_path.join("build.gradle").exists() || proj_path.join("build.gradle.kts").exists() || proj_path.join("pom.xml").exists() {
        profile.primary_languages.insert("kotlin".to_string());
        profile.primary_languages.insert("java".to_string());
    }
    profile
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_language_profile() {
        let files = vec![
            FileEntry {
                path: "src/main.rs".to_string(),
                file_type: "file".to_string(),
                size_bytes: 100,
            },
            FileEntry {
                path: "src/lib.rs".to_string(),
                file_type: "file".to_string(),
                size_bytes: 100,
            },
            FileEntry {
                path: "migrations/schema.sql".to_string(),
                file_type: "file".to_string(),
                size_bytes: 100,
            },
            FileEntry {
                path: "Dockerfile".to_string(),
                file_type: "file".to_string(),
                size_bytes: 100,
            },
        ];

        let profile = detect_language_profile(&files, "refactor database schema");
        assert!(profile.primary_languages.contains("rust"));
        assert!(profile.secondary_tech.contains("sql"));
        assert!(profile.secondary_tech.contains("database"));
        assert!(profile.secondary_tech.contains("docker"));

        let py_profile = detect_language_profile(&files, "write a python script");
        assert!(py_profile.primary_languages.contains("python"));
    }

    #[test]
    fn test_language_detector_avoids_word_and_extension_false_positives() {
        let files = vec![
            FileEntry {
                path: "go".to_string(),
                file_type: "file".to_string(),
                size_bytes: 10,
            },
            FileEntry {
                path: "scripts.py".to_string(),
                file_type: "dir".to_string(),
                size_bytes: 0,
            },
        ];
        let profile = detect_language_profile(
            &files,
            "implement a good javascript algorithm with trust",
        );
        assert!(profile.primary_languages.contains("javascript"));
        assert!(!profile.primary_languages.contains("java"));
        assert!(!profile.primary_languages.contains("go"));
        assert!(!profile.primary_languages.contains("rust"));
        assert!(!profile.primary_languages.contains("python"));

        let go_cpp = detect_language_profile(&[], "write a go and c++20 service");
        assert!(go_cpp.primary_languages.contains("go"));
        assert!(go_cpp.primary_languages.contains("c++"));
    }

    #[test]
    fn test_detect_language_fast_ignores_parent_docker_dir() {
        let tmp_parent = std::env::temp_dir()
            .join(format!("docker_parent_{}_{:?}", std::process::id(), std::thread::current().id()));
        let proj = tmp_parent.join("my_rust_app");
        let _ = std::fs::create_dir_all(proj.join("src"));
        let _ = std::fs::write(proj.join("src").join("main.rs"), "fn main() {}");

        let profile = detect_language_fast(&proj, "build binary");
        assert!(profile.primary_languages.contains("rust"));
        assert!(!profile.secondary_tech.contains("docker"));

        let _ = std::fs::remove_dir_all(&tmp_parent);
    }
}
