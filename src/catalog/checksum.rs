use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::store::SkillItem;

static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{:02x}", b);
    }
    s
}

pub fn compute_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    bytes_to_hex(&hasher.finalize())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillManifestEntry {
    pub name: String,
    pub sha256: String,
    pub section_count: usize,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillsManifest {
    pub catalog_hash: String,
    pub count: usize,
    pub created_at: u64,
    pub skills: HashMap<String, SkillManifestEntry>,
}

pub fn skills_manifest_path() -> PathBuf {
    if cfg!(test) {
        return std::env::temp_dir()
            .join(format!("ag_test_home_{}", std::process::id()))
            .join("skills_manifest.json");
    }
    dirs::home_dir()
        .map(|h| h.join(".agent-guidance").join("skills_manifest.json"))
        .unwrap_or_else(|| PathBuf::from(".agent-guidance").join("skills_manifest.json"))
}

pub fn load_manifest() -> Option<SkillsManifest> {
    let path = skills_manifest_path();
    let data = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

pub fn save_manifest(manifest: &SkillsManifest) -> anyhow::Result<()> {
    let path = skills_manifest_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(manifest)?;
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = path.with_extension(format!("tmp.{}.{}", std::process::id(), seq));
    std::fs::write(&tmp, json)?;
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(())
}

pub fn compute_catalog_hash(entries: &[SkillManifestEntry]) -> String {
    let mut sorted: Vec<&SkillManifestEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    let mut hasher = Sha256::new();
    for entry in sorted {
        hasher.update(entry.name.as_bytes());
        hasher.update(b":");
        hasher.update(entry.sha256.as_bytes());
        hasher.update(b";");
    }
    bytes_to_hex(&hasher.finalize())
}

#[derive(Debug, Default)]
pub struct CatalogDiff {
    pub modified_or_new: Vec<SkillItem>,
    pub unchanged: Vec<String>,
    pub deleted_slugs: Vec<String>,
}

pub fn detect_catalog_diff(skills: &[SkillItem], prev: Option<&SkillsManifest>) -> CatalogDiff {
    let mut diff = CatalogDiff::default();
    let prev_lower: Option<HashMap<String, &SkillManifestEntry>> = prev.map(|p| {
        p.skills
            .iter()
            .map(|(k, v)| (k.to_lowercase(), v))
            .collect()
    });

    for skill in skills {
        let hash = compute_sha256(skill.content.as_bytes());
        let norm = skill.name.to_lowercase();
        if let Some(entry) = prev_lower.as_ref().and_then(|map| map.get(&norm))
            && entry.sha256 == hash
        {
            diff.unchanged.push(norm);
            continue;
        }
        diff.modified_or_new.push(skill.clone());
    }

    if let Some(map) = prev_lower {
        let current_names: std::collections::HashSet<String> =
            skills.iter().map(|s| s.name.to_lowercase()).collect();
        for prev_name in map.keys() {
            if !current_names.contains(prev_name) {
                diff.deleted_slugs.push(prev_name.clone());
            }
        }
    }

    diff
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::store::SkillSource;

    #[test]
    fn test_compute_sha256_and_diff() {
        let h1 = compute_sha256(b"hello world");
        assert_eq!(h1.len(), 64);

        let skill1 = SkillItem {
            name: "test-skill".to_string(),
            relative_path: "test".to_string(),
            source: SkillSource::Embedded,
            content: "hello world".to_string(),
        };

        let diff = detect_catalog_diff(&[skill1], None);
        assert_eq!(diff.modified_or_new.len(), 1);
        assert_eq!(diff.unchanged.len(), 0);
    }

    #[test]
    fn test_detect_catalog_diff_case_insensitive_manifest_keys() {
        let content = "hello world";
        let hash = compute_sha256(content.as_bytes());
        let mut skills_map = HashMap::new();
        skills_map.insert(
            "MySkill".to_string(),
            SkillManifestEntry {
                name: "MySkill".to_string(),
                sha256: hash,
                section_count: 1,
                updated_at: 1,
            },
        );
        let manifest = SkillsManifest {
            catalog_hash: String::new(),
            count: 1,
            created_at: 1,
            skills: skills_map,
        };
        let current = SkillItem {
            name: "myskill".to_string(),
            relative_path: "myskill/SKILL.md".to_string(),
            source: SkillSource::Embedded,
            content: content.to_string(),
        };
        let diff = detect_catalog_diff(&[current], Some(&manifest));
        assert_eq!(diff.unchanged, vec!["myskill".to_string()]);
        assert!(diff.modified_or_new.is_empty());
        assert!(diff.deleted_slugs.is_empty());
    }
}
