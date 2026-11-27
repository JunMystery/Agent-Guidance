use rayon::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use tracing::info;

use crate::catalog::store::{SkillItem, SkillSource, get_embedded_skill, list_embedded_skills};
use crate::ml::inference_pool;
use super::gpu::GpuSkillMatrix;
use super::model::EmbeddingModel;
use super::precomputed::{
    catalog_fingerprint, load_passage_cache, load_precomputed_cache, load_precomputed_map,
    save_passage_cache,
};

#[derive(Clone)]
pub struct PassageCache {
    pub fingerprint: u64,
    pub vectors: Arc<Vec<Vec<f32>>>,
}

pub static PASSAGE_CACHE: OnceLock<RwLock<Vec<PassageCache>>> = OnceLock::new();
pub static GPU_SKILL_MATRIX: OnceLock<RwLock<Option<GpuSkillMatrix>>> = OnceLock::new();
static WARMUP_DONE: OnceLock<AtomicBool> = OnceLock::new();

pub fn store_passage_cache(vectors: Vec<Vec<f32>>, skills: &[SkillItem]) {
    let fingerprint = catalog_fingerprint(skills);
    let cache = PASSAGE_CACHE.get_or_init(|| RwLock::new(Vec::new()));
    if let Ok(mut guard) = cache.write() {
        guard.retain(|entry| entry.fingerprint != fingerprint);
        guard.push(PassageCache {
            fingerprint,
            vectors: Arc::new(vectors.clone()),
        });
    }

    let (device, dev_name) = super::device::resolve_optimal_device();
    if let Ok(gpu_matrix) = GpuSkillMatrix::from_vectors(&vectors, fingerprint, &device) {
        let gpu_slot = GPU_SKILL_MATRIX.get_or_init(|| RwLock::new(None));
        if let Ok(mut g_guard) = gpu_slot.write() {
            *g_guard = Some(gpu_matrix);
            info!(
                "[VRAM Residency] GpuSkillMatrix populated with {} skill vectors on {}.",
                vectors.len(),
                dev_name
            );
        }
    }
}

#[allow(dead_code)]
pub fn is_warmup_complete() -> bool {
    WARMUP_DONE
        .get_or_init(|| AtomicBool::new(false))
        .load(Ordering::Relaxed)
}

pub fn mark_warmup_complete() {
    WARMUP_DONE
        .get_or_init(|| AtomicBool::new(false))
        .store(true, Ordering::Relaxed);
}

static MODEL: OnceLock<Result<RwLock<EmbeddingModel>, String>> = OnceLock::new();

pub fn try_cached_model() -> Option<std::sync::RwLockReadGuard<'static, EmbeddingModel>> {
    MODEL.get().and_then(|res| res.as_ref().ok()).and_then(|rw| rw.try_read().ok())
}

pub fn cached_model() -> Result<std::sync::RwLockReadGuard<'static, EmbeddingModel>, String> {
    match MODEL.get_or_init(|| {
        EmbeddingModel::load_or_download()
            .map(RwLock::new)
            .map_err(|e| e.to_string())
    }) {
        Ok(model) => model.read().map_err(|_| "RwLock poisoned".to_string()),
        Err(err) => Err(err.clone()),
    }
}

pub fn warmup_cache() {
    let candidates: Vec<SkillItem> = list_embedded_skills()
        .iter()
        .filter_map(|path| {
            get_embedded_skill(path).map(|content| SkillItem {
                name: path.split('/').next().unwrap_or(path).to_string(),
                relative_path: path.clone(),
                source: SkillSource::Embedded,
                content,
            })
        })
        .collect();

    // 1. Precomputed cache (embedded in binary) — instant, no model needed
    if let Some(cached) = load_precomputed_cache(&candidates) {
        store_passage_cache(cached, &candidates);
        mark_warmup_complete();
        info!(
            "Loaded precomputed passage cache ({} skills).",
            candidates.len()
        );
        eager_load_embedding_model();
        return;
    }

    // 2. On-disk cache (~/.agent-guidance/vectors.bin)
    if let Some(cached) = load_passage_cache(&candidates) {
        store_passage_cache(cached, &candidates);
        mark_warmup_complete();
        info!(
            "Loaded passage cache from disk ({} skills).",
            candidates.len()
        );
        eager_load_embedding_model();
        return;
    }

    // 3. Reuse precomputed vectors for known skills and embed only missing skills on CPU
    info!(
        "Computing passage embeddings for {} skills...",
        candidates.len()
    );
    let precomputed_map = load_precomputed_map().map(|(_, m)| m);
    let missing_indices: Vec<usize> = candidates
        .iter()
        .enumerate()
        .filter_map(|(idx, cand)| {
            let found = precomputed_map
                .as_ref()
                .and_then(|m| m.get(&cand.name.to_lowercase()))
                .is_some();
            if found { None } else { Some(idx) }
        })
        .collect();

    let mut slots: Vec<Option<Vec<f32>>> = candidates
        .iter()
        .map(|cand| {
            precomputed_map
                .as_ref()
                .and_then(|m| m.get(&cand.name.to_lowercase()).cloned())
        })
        .collect();

    if !missing_indices.is_empty() {
        if let Ok(guard) = cached_model() {
            let missing_vecs: Vec<(usize, Option<Vec<f32>>)> = inference_pool().install(|| {
                missing_indices
                    .par_iter()
                    .map(|&idx| {
                        let text = candidates[idx].to_search_passage();
                        (idx, guard.embed_text(&text, Some("passage")).ok())
                    })
                    .collect()
            });
            for (idx, maybe_vec) in missing_vecs {
                slots[idx] = maybe_vec;
            }
        }
    }

    let vecs: Vec<Vec<f32>> = slots.into_iter().flatten().collect();
    if vecs.len() == candidates.len() {
        save_passage_cache(&vecs, &candidates);
        store_passage_cache(vecs, &candidates);
        mark_warmup_complete();
        info!(
            "Passage embedding warmup complete ({} skills).",
            candidates.len()
        );
    } else {
        tracing::warn!(
            "Passage embedding warmup incomplete ({}/{} skills embedded); skipping disk cache write.",
            vecs.len(),
            candidates.len()
        );
    }

    // 4. Preload models so first user query doesn't pay OnceLock init
    eager_load_embedding_model();
}

static AUTO_WARMUP_TRIGGERED: OnceLock<AtomicBool> = OnceLock::new();
static BG_EMBED_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

struct BgEmbedGuard;
impl Drop for BgEmbedGuard {
    fn drop(&mut self) {
        BG_EMBED_IN_FLIGHT.store(false, Ordering::SeqCst);
    }
}

pub fn spawn_background_auto_warmup() {
    let triggered = AUTO_WARMUP_TRIGGERED.get_or_init(|| AtomicBool::new(false));
    if triggered.swap(true, Ordering::SeqCst) {
        return;
    }

    if crate::config::current_config().server.is_remote() {
        info!("[ML Engine] Auto-ML warmup bypassed in Remote Client mode (Zero-ML Client Invariant)");
        return;
    }

    if std::env::var("AGENT_GUIDANCE_DISABLE_ML")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        info!("[ML Engine] Auto-ML warmup bypassed via AGENT_GUIDANCE_DISABLE_ML");
        return;
    }

    let _ = std::thread::Builder::new()
        .name("ag-ml-warmup".to_string())
        .spawn(|| {
            info!("[ML Engine] Background auto-warmup thread running...");
            warmup_cache();
            info!("[ML Engine] Background auto-warmup thread finished. Models active.");
        });
}

pub fn eager_load_embedding_model() {
    drop(cached_model());
    drop(crate::ml::llm_selector::cached_cross_encoder());
}

pub fn clear_passage_cache() {
    let cache = PASSAGE_CACHE.get_or_init(|| RwLock::new(Vec::new()));
    if let Ok(mut guard) = cache.write() {
        guard.clear();
    }
    let gpu_slot = GPU_SKILL_MATRIX.get_or_init(|| RwLock::new(None));
    if let Ok(mut g_guard) = gpu_slot.write() {
        *g_guard = None;
    }
}

pub fn embed_skills_cache(candidates: &[SkillItem], model: &EmbeddingModel) -> Arc<Vec<Vec<f32>>> {
    let fingerprint = catalog_fingerprint(candidates);
    let cache = PASSAGE_CACHE.get_or_init(|| RwLock::new(Vec::new()));
    if let Ok(guard) = cache.read() {
        if let Some(entry) = guard.iter().find(|entry| {
            entry.fingerprint == fingerprint && entry.vectors.len() == candidates.len()
        }) {
            return entry.vectors.clone();
        }
    }

    if let Some(cached) = load_precomputed_cache(candidates) {
        store_passage_cache(cached, candidates);
        if let Ok(guard) = cache.read() {
            if let Some(entry) = guard.iter().find(|e| e.fingerprint == fingerprint) {
                return entry.vectors.clone();
            }
        }
    }

    if let Some(cached) = load_passage_cache(candidates) {
        store_passage_cache(cached, candidates);
        if let Ok(guard) = cache.read() {
            if let Some(entry) = guard.iter().find(|e| e.fingerprint == fingerprint) {
                return entry.vectors.clone();
            }
        }
    }

    // Reuse precomputed vectors for known skills and embed only missing skills synchronously if <= 64
    let precomputed_map = load_precomputed_map().map(|(_, m)| m);
    let missing_indices: Vec<usize> = candidates
        .iter()
        .enumerate()
        .filter_map(|(idx, cand)| {
            let found = precomputed_map
                .as_ref()
                .and_then(|m| m.get(&cand.name.to_lowercase()))
                .is_some();
            if found { None } else { Some(idx) }
        })
        .collect();

    if missing_indices.len() > 64 {
        if !BG_EMBED_IN_FLIGHT.swap(true, Ordering::SeqCst) {
            info!(
                "[ML Pipeline] Deferring on-the-fly embedding of {} skills to background to avoid timeout; using fast semantic fallback.",
                missing_indices.len()
            );
            let cand_owned = candidates.to_vec();
            if std::thread::Builder::new()
                .name("ag-passage-bg".to_string())
                .spawn(move || {
                    let _guard = BgEmbedGuard;
                    if let Ok(model_guard) = cached_model() {
                        let pre_map = load_precomputed_map().map(|(_, m)| m);
                        let mut vecs = Vec::with_capacity(cand_owned.len());
                        for cand in &cand_owned {
                            if let Some(v) = pre_map.as_ref().and_then(|m| m.get(&cand.name.to_lowercase())) {
                                vecs.push(v.clone());
                            } else if let Ok(v) = model_guard.embed_text(&cand.to_search_passage(), Some("passage")) {
                                vecs.push(v);
                            }
                        }
                        if vecs.len() == cand_owned.len() {
                            save_passage_cache(&vecs, &cand_owned);
                            store_passage_cache(vecs, &cand_owned);
                        }
                    }
                })
                .is_err()
            {
                BG_EMBED_IN_FLIGHT.store(false, Ordering::SeqCst);
            }
        }
        return Arc::new(Vec::new());
    }

    info!(
        "[ML Pipeline] Computing passage embeddings for {} missing skill(s) on-the-fly (of {} total)...",
        missing_indices.len(),
        candidates.len()
    );
    let missing_vecs: Vec<(usize, Option<Vec<f32>>)> = inference_pool().install(|| {
        missing_indices
            .par_iter()
            .map(|&idx| {
                let text = candidates[idx].to_search_passage();
                (idx, model.embed_text(&text, Some("passage")).ok())
            })
            .collect()
    });
    let mut vecs: Vec<Option<Vec<f32>>> = candidates
        .iter()
        .map(|cand| {
            precomputed_map
                .as_ref()
                .and_then(|m| m.get(&cand.name.to_lowercase()).cloned())
        })
        .collect();
    for (idx, maybe_vec) in missing_vecs {
        vecs[idx] = maybe_vec;
    }
    let vecs: Vec<Vec<f32>> = vecs.into_iter().flatten().collect();
    if vecs.len() != candidates.len() {
        return Arc::new(Vec::new());
    }
    save_passage_cache(&vecs, candidates);
    store_passage_cache(vecs, candidates);
    cache
        .read()
        .ok()
        .and_then(|guard| {
            guard
                .iter()
                .find(|entry| entry.fingerprint == fingerprint)
                .map(|entry| entry.vectors.clone())
        })
        .unwrap_or_else(|| Arc::new(Vec::new()))
}

