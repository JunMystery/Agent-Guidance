use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

static TMP_SEQ: AtomicU64 = AtomicU64::new(0);
static TOMBSTONE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn tombstone_mutex() -> &'static Mutex<()> {
    TOMBSTONE_LOCK.get_or_init(|| Mutex::new(()))
}

pub fn tombstones_path() -> PathBuf {
    if cfg!(test) {
        return std::env::temp_dir()
            .join(format!("ag_test_home_{}", std::process::id()))
            .join("tombstones.json");
    }
    dirs::home_dir()
        .map(|h| h.join(".agent-guidance").join("tombstones.json"))
        .unwrap_or_else(|| PathBuf::from(".agent-guidance").join("tombstones.json"))
}

fn load_tombstones_strict_from_path(path: &Path) -> anyhow::Result<HashSet<String>> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(serde_json::from_str::<HashSet<String>>(&content)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(HashSet::new()),
        Err(e) => Err(e.into()),
    }
}

fn save_tombstones_to_path(path: &Path, tombstones: &HashSet<String>) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(tombstones)?;
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp_path = path.with_extension(format!("tmp.{}.{}", std::process::id(), seq));
    std::fs::write(&tmp_path, json)?;
    if let Err(e) = std::fs::rename(&tmp_path, path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e.into());
    }
    Ok(())
}

pub fn load_tombstones() -> HashSet<String> {
    let path = tombstones_path();
    load_tombstones_strict_from_path(&path).unwrap_or_default()
}

pub fn add_tombstones<I, S>(slugs: I) -> anyhow::Result<usize>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let _guard = tombstone_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let path = tombstones_path();
    let mut current = load_tombstones_strict_from_path(&path)?;
    let mut added = 0;
    for slug in slugs {
        let norm = slug.as_ref().trim().to_lowercase();
        if !norm.is_empty() && current.insert(norm) {
            added += 1;
        }
    }
    if added > 0 {
        save_tombstones_to_path(&path, &current)?;
    }
    Ok(added)
}

pub fn remove_tombstones<I, S>(slugs: I) -> anyhow::Result<usize>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let _guard = tombstone_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let path = tombstones_path();
    let mut current = load_tombstones_strict_from_path(&path)?;
    let mut removed = 0;
    for slug in slugs {
        let norm = slug.as_ref().trim().to_lowercase();
        if current.remove(&norm) {
            removed += 1;
        }
    }
    if removed > 0 {
        save_tombstones_to_path(&path, &current)?;
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

    #[test]
    fn test_strict_load_errors_on_corrupt_json_instead_of_silently_wiping() {
        let tmp_dir = std::env::temp_dir().join(format!(
            "test_tombstone_corrupt_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::create_dir_all(&tmp_dir);
        let corrupt_file = tmp_dir.join("tombstones.json");
        std::fs::write(&corrupt_file, "{not valid json").unwrap();

        assert!(load_tombstones_strict_from_path(&corrupt_file).is_err());

        let missing_file = tmp_dir.join("missing.json");
        assert!(load_tombstones_strict_from_path(&missing_file).unwrap().is_empty());

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn test_concurrent_add_tombstones_preserves_all_updates() {
        let handles: Vec<_> = (0..6)
            .map(|i| {
                std::thread::spawn(move || {
                    let slug = format!("concurrent-tomb-{}", i);
                    add_tombstones([slug.as_str()]).unwrap();
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let current = load_tombstones();
        for i in 0..6 {
            let slug = format!("concurrent-tomb-{}", i);
            assert!(current.contains(&slug), "lost concurrent tombstone {}", slug);
        }
        let cleanup: Vec<String> = (0..6).map(|i| format!("concurrent-tomb-{}", i)).collect();
        let _ = remove_tombstones(cleanup);
    }
}
