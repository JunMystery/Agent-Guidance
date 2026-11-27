use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

const DEFAULT_CACHE_TTL_SECS: u64 = 30;

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct AstCacheKey {
    pub path: String,
    pub target_symbol: Option<String>,
    pub view_mode: String,
    pub start_line: Option<usize>,
    pub end_line: Option<usize>,
    pub show_line_numbers: bool,
}

#[derive(Clone, Debug)]
pub struct AstCacheEntry {
    pub rendered_body: String,
    pub disk_mtime: SystemTime,
    pub file_size: u64,
    pub cached_at: Instant,
    pub baseline_tokens: u64,
}

#[derive(Clone, Debug)]
pub struct AstResponseCache {
    entries: Arc<Mutex<HashMap<AstCacheKey, AstCacheEntry>>>,
    capacity: usize,
}

impl Default for AstResponseCache {
    fn default() -> Self {
        Self::new(100)
    }
}

impl AstResponseCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: Arc::new(Mutex::new(HashMap::new())),
            capacity: capacity.max(10),
        }
    }

    pub fn get(&self, key: &AstCacheKey, full_path: &Path) -> Option<(String, u64, u64)> {
        let guard = self.entries.lock().ok()?;
        let entry = guard.get(key)?;
        let elapsed = entry.cached_at.elapsed();
        if elapsed > Duration::from_secs(DEFAULT_CACHE_TTL_SECS) {
            return None;
        }

        let meta = fs::metadata(full_path).ok()?;
        let mtime = meta.modified().ok()?;
        let file_size = meta.len();
        if mtime != entry.disk_mtime || file_size != entry.file_size {
            return None;
        }

        Some((
            entry.rendered_body.clone(),
            elapsed.as_secs(),
            entry.baseline_tokens,
        ))
    }

    pub fn insert(
        &mut self,
        key: AstCacheKey,
        rendered_body: String,
        full_path: &Path,
        tokens: u64,
    ) {
        let Ok(meta) = fs::metadata(full_path) else {
            return;
        };
        let Ok(mtime) = meta.modified() else {
            return;
        };
        let file_size = meta.len();

        let Ok(mut guard) = self.entries.lock() else {
            return;
        };

        if guard.len() >= self.capacity {
            guard.retain(|_, v| v.cached_at.elapsed() < Duration::from_secs(DEFAULT_CACHE_TTL_SECS));
            if guard.len() >= self.capacity {
                let to_evict = (guard.len() - self.capacity + 1).max(self.capacity / 4);
                let mut by_age: Vec<(Instant, AstCacheKey)> = guard
                    .iter()
                    .map(|(k, v)| (v.cached_at, k.clone()))
                    .collect();
                by_age.sort_by_key(|(ts, _)| *ts);
                for (_, old_key) in by_age.into_iter().take(to_evict) {
                    guard.remove(&old_key);
                }
            }
        }

        guard.insert(
            key,
            AstCacheEntry {
                rendered_body,
                disk_mtime: mtime,
                file_size,
                cached_at: Instant::now(),
                baseline_tokens: tokens,
            },
        );
    }

    pub fn invalidate_file(&mut self, rel_path: &str) {
        let raw = rel_path.trim().replace('\\', "/");
        let norm = raw.trim_start_matches("./").trim_start_matches('/');
        if norm.is_empty() {
            return;
        }
        let boundary_suffix = format!("/{}", norm);
        if let Ok(mut guard) = self.entries.lock() {
            guard.retain(|k, _| {
                k.path != raw && k.path != norm && !k.path.ends_with(&boundary_suffix)
            });
        }
    }

    #[allow(dead_code)]
    pub fn clear(&mut self) {
        if let Ok(mut guard) = self.entries.lock() {
            guard.clear();
        }
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.entries.lock().map(|g| g.len()).unwrap_or(0)
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.entries.lock().map(|g| g.is_empty()).unwrap_or(true)
    }
}
