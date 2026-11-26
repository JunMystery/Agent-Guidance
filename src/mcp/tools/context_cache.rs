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

        let mtime = fs::metadata(full_path).ok()?.modified().ok()?;
        if mtime != entry.disk_mtime {
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
        let mtime = match fs::metadata(full_path).ok().and_then(|m| m.modified().ok()) {
            Some(t) => t,
            None => return,
        };

        let Ok(mut guard) = self.entries.lock() else {
            return;
        };

        if guard.len() >= self.capacity {
            guard.retain(|_, v| v.cached_at.elapsed() < Duration::from_secs(DEFAULT_CACHE_TTL_SECS));
            if guard.len() >= self.capacity {
                guard.clear();
            }
        }

        guard.insert(
            key,
            AstCacheEntry {
                rendered_body,
                disk_mtime: mtime,
                cached_at: Instant::now(),
                baseline_tokens: tokens,
            },
        );
    }

    pub fn invalidate_file(&mut self, rel_path: &str) {
        let norm = rel_path.trim().replace('\\', "/");
        if norm.is_empty() {
            return;
        }
        if let Ok(mut guard) = self.entries.lock() {
            guard.retain(|k, _| k.path != norm && !k.path.ends_with(&norm));
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
