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

    let model_status = if let Some(vecs) = crate::ml::embeddings::precomputed::load_precomputed_cache(&all_skills)
        .or_else(|| crate::ml::embeddings::load_passage_cache(&all_skills))
    {
        format!(
            "Loaded cached embeddings for {} skills (dimension: {})",
            vecs.len(),
            vecs.first().map(|v| v.len()).unwrap_or(0)
        )
    } else {
        "Metadata indexed (embeddings loaded on-demand)".to_string()
    };

    Ok(format!(
        "# Skill Semantic Index Refreshed\n\n- Total Skills: {}\n- Catalog Fingerprint: {:016x}\n- Status: {}\n- Cache Path: `~/.agent-guidance/vectors.bin`\n\nAll workspace and embedded skills reindexed with rich semantic passages.",
        count, fp, model_status
    ))
}
