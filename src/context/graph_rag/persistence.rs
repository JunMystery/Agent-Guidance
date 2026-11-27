use anyhow::Result;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use super::community::CommunityHierarchy;

static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

fn atomic_write(dir: &Path, tmp_prefix: &str, target: &Path, content: &str) -> Result<()> {
    let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp_path = dir.join(format!("{}.tmp.{}.{}", tmp_prefix, std::process::id(), seq));
    if let Err(err) = fs::write(&tmp_path, content) {
        let _ = fs::remove_file(&tmp_path);
        return Err(err.into());
    }
    if let Err(err) = fs::rename(&tmp_path, target) {
        let _ = fs::remove_file(&tmp_path);
        return Err(err.into());
    }
    Ok(())
}

pub fn communities_file_path(project_path: &Path) -> std::path::PathBuf {
    project_path.join(".agent-context").join("communities.json")
}

pub fn save_hierarchy(project_path: &Path, hierarchy: &CommunityHierarchy) -> Result<()> {
    let dir = project_path.join(".agent-context");
    fs::create_dir_all(&dir)?;

    let file_path = communities_file_path(project_path);
    let json = serde_json::to_string_pretty(hierarchy)?;
    atomic_write(&dir, "communities", &file_path, &json)?;

    // Also update .agent-context/architecture.json with detected architecture
    let arch_file = dir.join("architecture.json");
    let payload = serde_json::json!({
        "architecture_pattern": hierarchy.detected_architecture,
        "updated_at": hierarchy.updated_at,
        "community_count": hierarchy.communities.len()
    });
    let arch_json = serde_json::to_string_pretty(&payload)?;
    atomic_write(&dir, "architecture", &arch_file, &arch_json)?;

    Ok(())
}

pub fn load_hierarchy(project_path: &Path) -> Option<CommunityHierarchy> {
    let file_path = communities_file_path(project_path);
    if file_path.exists() {
        if let Ok(content) = fs::read_to_string(&file_path) {
            if let Ok(hierarchy) = serde_json::from_str::<CommunityHierarchy>(&content) {
                return Some(hierarchy);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_save_hierarchy_atomic_writes_both_files_without_leaking_temps() {
        let dir = std::env::temp_dir().join(format!(
            "ag_persistence_test_{}_{}",
            std::process::id(),
            TEMP_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let h1 = CommunityHierarchy::new("demo", "Clean Architecture");
        save_hierarchy(&dir, &h1).unwrap();

        let h2 = CommunityHierarchy::new("demo", "Hexagonal");
        save_hierarchy(&dir, &h2).unwrap();

        let loaded = load_hierarchy(&dir).expect("hierarchy should load");
        assert_eq!(loaded.detected_architecture, "Hexagonal");

        let arch_raw = fs::read_to_string(dir.join(".agent-context").join("architecture.json")).unwrap();
        assert!(arch_raw.contains("Hexagonal"));

        let leftover_temps: Vec<_> = fs::read_dir(dir.join(".agent-context"))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
            .collect();
        assert!(leftover_temps.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }
}
