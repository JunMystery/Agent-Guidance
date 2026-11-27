use serde_json::Value;

use crate::catalog::store::load_all_skills;
use crate::mcp::state::ServerState;
use super::helpers::detect_project_path;

pub(crate) fn handle_reindex(
    arguments: &Value,
    state: &mut ServerState,
) -> Result<String, (i32, String)> {
    let proj_path_arg = arguments
        .get("project_path")
        .and_then(|p| p.as_str())
        .unwrap_or(".");
    let proj_path = detect_project_path(proj_path_arg, state);

    crate::ml::embeddings::cache::clear_passage_cache();
    crate::context::cache::invalidate_snapshot(&proj_path);

    let all_skills = load_all_skills(&proj_path);
    let count = all_skills.len();
    let fp = crate::ml::embeddings::catalog_fingerprint(&all_skills);

    let model_status = if let Some(vecs) = crate::ml::embeddings::precomputed::load_precomputed_cache(&all_skills) {
        let dim = vecs.first().map(|v| v.len()).unwrap_or(0);
        crate::ml::embeddings::cache::store_passage_cache(vecs, &all_skills);
        format!(
            "Reloaded precomputed embeddings for {} skills (dimension: {})",
            count, dim
        )
    } else if let Some(vecs) = crate::ml::embeddings::load_passage_cache(&all_skills) {
        let dim = vecs.first().map(|v| v.len()).unwrap_or(0);
        crate::ml::embeddings::cache::store_passage_cache(vecs, &all_skills);
        format!(
            "Reloaded on-disk passage cache for {} skills (dimension: {})",
            count, dim
        )
    } else if let Some(model) = crate::ml::embeddings::try_cached_model() {
        let rebuilt = crate::ml::embeddings::cache::embed_skills_cache(&all_skills, &model);
        if rebuilt.len() == count && count > 0 {
            format!(
                "Rebuilt and persisted passage embeddings for {} skills (dimension: {})",
                rebuilt.len(),
                rebuilt.first().map(|v| v.len()).unwrap_or(0)
            )
        } else {
            "Metadata indexed (passage_vectors.bin rebuild deferred)".to_string()
        }
    } else {
        "Metadata indexed (embedding model not loaded; passage_vectors.bin rebuild on-demand)".to_string()
    };

    Ok(format!(
        "# Skill Semantic Index Refreshed\n\n- Total Skills: {}\n- Catalog Fingerprint: {:016x}\n- Status: {}\n- Cache Path: `{}`\n\nAll workspace and embedded skills reindexed with rich semantic passages.",
        count,
        fp,
        model_status,
        crate::ml::embeddings::precomputed::passage_vectors_path().display()
    ))
}
