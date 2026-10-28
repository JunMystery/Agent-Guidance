use std::path::Path;
use crate::catalog::language_detector::{ProjectLanguageProfile, detect_language_fast};
use crate::catalog::store::SkillItem;
use crate::ml::embeddings::hybrid_vector_search;
use crate::ml::llm_selector::LLMSelector;
use super::guidance_intent::is_tooling_skill;

/// Two-Stage Hybrid Skill Discovery
/// Stage 1: Clean Core Prompt Search
/// Stage 2: Project Tech Stack & Languages Expansion (only if Stage 1 yields low confidence)
pub fn discover_skills_two_stage(
    proj_path: &Path,
    search_query: &str,
    all_skills: &[SkillItem],
    mentioned_skills: &[String],
) -> (Vec<(f32, SkillItem)>, ProjectLanguageProfile) {
    let profile = detect_language_fast(proj_path, search_query);
    let selector = LLMSelector::new();

    // Stage 1: Search on pure user prompt without historical task/file pollution
    let stage1_vector = hybrid_vector_search(search_query, all_skills, 20);
    let stage1_gated = crate::ml::skill_graphrag_gate::apply_graphrag_context_gating(
        proj_path,
        search_query,
        stage1_vector,
        20,
    );
    let stage1_reranked = selector.rerank(search_query, stage1_gated, &profile, 20);

    let high_conf_count = stage1_reranked.iter().filter(|(score, _)| *score >= 0.70).count();

    // Stage 2: Only expand if Stage 1 found SOME relevant skill (1 match) but < 2 high confidence skills,
    // to prevent hallucinating repo-language skills on non-action conversational prompts.
    let final_candidates = if high_conf_count == 1 && !profile.primary_languages.is_empty() {
        let tech_suffix = profile.primary_languages.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" ");
        let expanded_query = format!("{} {}", search_query, tech_suffix);
        let stage2_vector = hybrid_vector_search(&expanded_query, all_skills, 20);
        let stage2_gated = crate::ml::skill_graphrag_gate::apply_graphrag_context_gating(
            proj_path,
            &expanded_query,
            stage2_vector,
            20,
        );
        let stage2_reranked = selector.rerank(search_query, stage2_gated, &profile, 20);

        // Merge stage1 and stage2 (preferring higher scores)
        let mut merged = stage1_reranked;
        for (score2, item2) in stage2_reranked {
            if let Some(existing) = merged.iter_mut().find(|(_, item)| item.name == item2.name) {
                if score2 > existing.0 {
                    existing.0 = score2;
                }
            } else {
                merged.push((score2, item2));
            }
        }
        merged.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        merged
    } else {
        stage1_reranked
    };

    let boosted = crate::ml::skill_analytics::apply_analytics_boost(final_candidates, proj_path);

    // Prioritize explicitly mentioned skills to the front with score 1.0
    let mut seen = std::collections::HashSet::new();
    let mut prioritized = Vec::new();
    for mentioned in mentioned_skills {
        if let Some(item) = all_skills.iter().find(|s| s.name == *mentioned) {
            if seen.insert(item.name.clone()) {
                prioritized.push((1.0, item.clone()));
            }
        }
    }
    for (score, item) in boosted {
        if seen.insert(item.name.clone()) {
            prioritized.push((score, item));
        }
    }

    (prioritized, profile)
}

/// Tiers proposal results: Domain skills first (Recommended), Tooling skills (Git, CI, Docs) second
pub fn tier_skill_results(
    results: Vec<(f32, SkillItem)>,
    mentioned_skills: &[String],
    limit: usize,
) -> Vec<(f32, SkillItem)> {
    let mut seen = std::collections::HashSet::new();
    let mut domain_skills = Vec::new();
    let mut tooling_skills = Vec::new();

    for (score, item) in results {
        if score < 0.65 || !seen.insert(item.name.clone()) {
            continue;
        }
        if mentioned_skills.contains(&item.name) {
            domain_skills.insert(0, (score, item));
        } else if is_tooling_skill(&item.name) {
            tooling_skills.push((score, item));
        } else {
            domain_skills.push((score, item));
        }
    }

    // Combine: Domain first, then Tooling, capped at limit
    let mut combined = domain_skills;
    combined.extend(tooling_skills);
    combined.truncate(limit);
    combined
}
