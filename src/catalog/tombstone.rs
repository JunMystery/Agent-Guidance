use std::collections::HashSet;
use std::path::PathBuf;

pub fn tombstones_path() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".agent-guidance").join("tombstones.json"))
        .unwrap_or_else(|| PathBuf::from(".agent-guidance").join("tombstones.json"))
}

pub fn load_tombstones() -> HashSet<String> {
    let path = tombstones_path();
    if !path.exists() {
        return HashSet::new();
    }
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str::<HashSet<String>>(&content).unwrap_or_default(),
        Err(_) => HashSet::new(),
    }
}

pub fn save_tombstones(tombstones: &HashSet<String>) -> anyhow::Result<()> {
    let path = tombstones_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(tombstones)?;
    let tmp_path = path.with_extension("tmp");
    std::fs::write(&tmp_path, json)?;
    std::fs::rename(tmp_path, path)?;
    Ok(())
}

pub fn add_tombstones<I, S>(slugs: I) -> anyhow::Result<usize>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut current = load_tombstones();
    let mut added = 0;
    for slug in slugs {
        let norm = slug.as_ref().trim().to_lowercase();
        if !norm.is_empty() && current.insert(norm) {
            added += 1;
        }
    }
    if added > 0 {
        save_tombstones(&current)?;
    }
    Ok(added)
}

pub fn remove_tombstones<I, S>(slugs: I) -> anyhow::Result<usize>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut current = load_tombstones();
    let mut removed = 0;
    for slug in slugs {
        let norm = slug.as_ref().trim().to_lowercase();
        if current.remove(&norm) {
            removed += 1;
        }
    }
    if removed > 0 {
        save_tombstones(&current)?;
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tombstone_serialization() {
        let mut set = HashSet::new();
        set.insert("skill-a".to_string());
        set.insert("skill-b".to_string());
        let json = serde_json::to_string(&set).unwrap();
        let loaded: HashSet<String> = serde_json::from_str(&json).unwrap();
        assert_eq!(set, loaded);
    }

    #[test]
    fn test_add_and_remove_tombstones() {
        let _ = add_tombstones(&["test-tomb-x", "test-tomb-y"]);
        let current = load_tombstones();
        assert!(current.contains("test-tomb-x"));
        assert!(current.contains("test-tomb-y"));

        let _ = remove_tombstones(&["test-tomb-x", "test-tomb-y"]);
        let after = load_tombstones();
        assert!(!after.contains("test-tomb-x"));
        assert!(!after.contains("test-tomb-y"));
    }
}
