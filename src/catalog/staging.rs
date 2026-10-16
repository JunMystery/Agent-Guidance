use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

use super::store::scanner::scan_skill_dir_recursive;
use super::store::{SkillItem, SkillSource};

pub fn staging_dir() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".agent-guidance").join("staging").join("skills"))
        .unwrap_or_else(|| PathBuf::from(".agent-guidance").join("staging").join("skills"))
}

pub fn ensure_staging_dir() -> anyhow::Result<PathBuf> {
    let dir = staging_dir();
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn scan_staging_skills() -> Vec<SkillItem> {
    let dir = staging_dir();
    if !dir.exists() || !dir.is_dir() {
        return Vec::new();
    }
    let mut results = Vec::new();
    scan_skill_dir_recursive(&dir, &dir, &mut results);
    // Mark source as Staged
    for item in &mut results {
        if let SkillSource::LocalWorkspace(path) = &item.source {
            item.relative_path = format!("staging/{}", item.name);
            item.source = SkillSource::LocalWorkspace(path.clone());
        }
    }
    results
}

pub fn is_valid_skill_slug(slug: &str) -> bool {
    let norm = slug.trim();
    !norm.is_empty()
        && norm
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

pub fn purge_staging_skill(slug: &str) -> anyhow::Result<bool> {
    if !is_valid_skill_slug(slug) {
        return Ok(false);
    }
    let dir = staging_dir();
    let norm = slug.trim().to_lowercase();

    let target_dir = dir.join(&norm);
    if target_dir.exists() && target_dir.is_dir() {
        fs::remove_dir_all(&target_dir)?;
        return Ok(true);
    }

    // Also check for direct markdown file e.g. skill-name.md
    let target_file = dir.join(format!("{}.md", norm));
    if target_file.exists() && target_file.is_file() {
        fs::remove_file(&target_file)?;
        return Ok(true);
    }

    Ok(false)
}

pub fn save_staging_skill(slug: &str, content: &str) -> anyhow::Result<()> {
    if !is_valid_skill_slug(slug) {
        anyhow::bail!("Invalid skill slug format: '{}'", slug);
    }
    let dir = ensure_staging_dir()?;
    let norm = slug.trim().to_lowercase();
    let target_dir = dir.join(&norm);
    fs::create_dir_all(&target_dir)?;
    fs::write(target_dir.join("SKILL.md"), content)?;

    // Auto-revive if skill was tombstoned
    let _ = crate::catalog::tombstone::remove_tombstones(&[norm.as_str()]);
    Ok(())
}

pub fn get_skill_content(name: &str) -> Option<(String, &'static str)> {
    if !is_valid_skill_slug(name) {
        return None;
    }
    let norm = name.trim().to_lowercase();
    let s_file = staging_dir().join(&norm).join("SKILL.md");
    if let Ok(c) = fs::read_to_string(s_file) {
        return Some((c, "staging"));
    }
    let s_flat = staging_dir().join(format!("{}.md", norm));
    if let Ok(c) = fs::read_to_string(s_flat) {
        return Some((c, "staging"));
    }
    if let Some(c) = crate::catalog::store::get_embedded_skill(&format!("{}/SKILL.md", norm)) {
        return Some((c, "embedded"));
    }
    if let Some(c) = crate::catalog::store::get_embedded_skill(&norm) {
        return Some((c, "embedded"));
    }
    None
}

/// Applies staged precedence over embedded skills, and filters out tombstoned skills.
pub fn merge_with_precedence(
    embedded: Vec<SkillItem>,
    staged: Vec<SkillItem>,
    tombstones: &HashSet<String>,
) -> Vec<SkillItem> {
    let mut map: HashMap<String, SkillItem> = HashMap::with_capacity(embedded.len() + staged.len());

    // 1. Insert embedded skills that are not tombstoned
    for item in embedded {
        let key = item.name.to_lowercase();
        if !tombstones.contains(&key) {
            map.insert(key, item);
        }
    }

    // 2. Insert or override with staged skills (staged skills take absolute precedence)
    for item in staged {
        let key = item.name.to_lowercase();
        if !tombstones.contains(&key) {
            map.insert(key, item);
        }
    }

    let mut merged: Vec<SkillItem> = map.into_values().collect();
    merged.sort_by(|a, b| a.name.cmp(&b.name));
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_precedence_and_tombstones() {
        let embedded = vec![
            SkillItem {
                name: "skill-a".to_string(),
                relative_path: "skills/skill-a/SKILL.md".to_string(),
                source: SkillSource::Embedded,
                content: "original-a".to_string(),
            },
            SkillItem {
                name: "skill-b".to_string(),
                relative_path: "skills/skill-b/SKILL.md".to_string(),
                source: SkillSource::Embedded,
                content: "original-b".to_string(),
            },
        ];

        let staged = vec![
            SkillItem {
                name: "skill-a".to_string(),
                relative_path: "staging/skill-a".to_string(),
                source: SkillSource::LocalWorkspace("/path".to_string()),
                content: "staged-a".to_string(),
            },
            SkillItem {
                name: "skill-c".to_string(),
                relative_path: "staging/skill-c".to_string(),
                source: SkillSource::LocalWorkspace("/path-c".to_string()),
                content: "staged-c".to_string(),
            },
        ];

        let mut tombstones = HashSet::new();
        tombstones.insert("skill-b".to_string());

        let merged = merge_with_precedence(embedded, staged, &tombstones);
        assert_eq!(merged.len(), 2);

        let skill_a = merged.iter().find(|s| s.name == "skill-a").unwrap();
        assert_eq!(skill_a.content, "staged-a");

        let skill_c = merged.iter().find(|s| s.name == "skill-c").unwrap();
        assert_eq!(skill_c.content, "staged-c");

        assert!(merged.iter().all(|s| s.name != "skill-b"));
    }

    #[test]
    fn test_save_and_get_staging_skill() {
        let slug = "test-temp-skill-xyz";
        let content = "# Temp Skill\nTest content";
        let res = save_staging_skill(slug, content);
        assert!(res.is_ok());

        let loaded = get_skill_content(slug);
        assert!(loaded.is_some());
        let (body, source) = loaded.unwrap();
        assert_eq!(body, content);
        assert_eq!(source, "staging");

        let purged = purge_staging_skill(slug);
        assert!(purged.unwrap());
        assert!(get_skill_content(slug).is_none());
    }
}
