use serde_json::Value;

use crate::catalog::language_detector::detect_language_fast;
use crate::catalog::slicing::slice_skill_markdown;
use crate::catalog::store::{get_embedded_skill, load_all_skills};
use crate::mcp::state::ServerState;
use crate::ml::embeddings::hybrid_vector_search;
use crate::ml::llm_selector::LLMSelector;
use crate::optimizer::compressor::compress_markdown;
use super::helpers::{detect_project_path, validate_path};

pub(crate) fn handle_get(
    arguments: &Value,
    state: &mut ServerState,
) -> Result<String, (i32, String)> {
    let id = arguments
        .get("identifier")
        .or_else(|| arguments.get("topic"))
        .and_then(|i| i.as_str())
        .unwrap_or("");
    let proj_path_arg = arguments
        .get("project_path")
        .and_then(|p| p.as_str())
        .unwrap_or(".");
    let proj_path = detect_project_path(proj_path_arg, state);

    let task_context = arguments
        .get("task")
        .or_else(|| arguments.get("query"))
        .or_else(|| arguments.get("context"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let format_skill = |content: &str| -> String {
        if !task_context.is_empty() {
            slice_skill_markdown(content, task_context, 3)
        } else {
            compress_markdown(content)
        }
    };

    let cfg = crate::config::current_config();
    if cfg.server.is_remote() {
        let client = crate::client::default_client();
        let clean_id = crate::mcp::tools::skills::clean_skill_identifier(id);
        if let Ok(slice) = client.slice_skill(&clean_id, task_context) {
            if !slice.content.trim().is_empty() {
                if !id.is_empty() {
                    crate::mcp::db::log_skill_load(id);
                }
                return Ok(slice.content);
            }
        }
    }

    if let Some((_, content, _)) = crate::mcp::tools::skills_resolver::resolve_skill(
        id,
        &[],
        &load_all_skills(&proj_path),
        &proj_path,
    ) {
        if !id.is_empty() {
            crate::mcp::db::log_skill_load(id);
        }
        Ok(format_skill(&content))
    } else if let Some(content) = get_embedded_skill(id) {
        if !id.is_empty() {
            crate::mcp::db::log_skill_load(id);
        }
        Ok(format_skill(&content))
    } else {
        Ok(format!("Skill asset not found: {}", id))
    }
}

pub(crate) fn handle_docs(
    arguments: &Value,
    query: &str,
    state: &mut ServerState,
) -> Result<String, (i32, String)> {
    let id = arguments
        .get("identifier")
        .or_else(|| arguments.get("topic"))
        .and_then(|i| i.as_str())
        .unwrap_or("general");

    let proj_path_arg = arguments
        .get("project_path")
        .and_then(|p| p.as_str())
        .unwrap_or(".");
    let proj_path = detect_project_path(proj_path_arg, state);
    let search_term = if !query.is_empty() { query } else { id };

    let cfg = crate::config::current_config();
    if cfg.server.is_remote() {
        let client = crate::client::default_client();
        match client.search_skills(search_term, 3) {
            Ok(remote_res) => {
                let mut docs_sections = Vec::new();
                for hit in remote_res.skills {
                    if let Ok(s) = client.slice_skill(&hit.name, search_term) {
                        if !s.content.trim().is_empty() {
                            docs_sections.push(format!(
                                "### Doc Skill: {}\nPath: skills/{}/SKILL.md\n\n{}",
                                hit.name, hit.name, s.content
                            ));
                        }
                    }
                }
                if !docs_sections.is_empty() {
                    return Ok(format!(
                        "# Documentation Guidance for '{}'\n\nQuery: '{}'\n\n{}",
                        id,
                        search_term,
                        docs_sections.join("\n\n---\n\n")
                    ));
                }
            }
            Err(e) => {
                tracing::warn!("Remote docs search failed: {}; attempting local fallback", e);
            }
        }
    }

    let all_skills = load_all_skills(&proj_path);
    let stage1 = hybrid_vector_search(search_term, &all_skills, 5);
    let selector = LLMSelector::new();
    let profile = detect_language_fast(&proj_path, search_term);
    let reranked = selector.rerank(search_term, stage1, &profile, 3);

    let mut docs_sections = Vec::new();
    for (_score, item) in reranked {
        let raw_content = validate_path(&proj_path, &format!(".agents/skills/{}/SKILL.md", item.name))
            .ok()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .or_else(|| {
                validate_path(&proj_path, &item.relative_path)
                    .ok()
                    .and_then(|p| std::fs::read_to_string(p).ok())
            })
            .or_else(|| get_embedded_skill(&item.relative_path));

        if let Some(content) = raw_content {
            // Surgical slicing extracts top relevant sections, saving ~75% tokens
            let sliced = slice_skill_markdown(&content, search_term, 3);
            if !sliced.trim().is_empty() {
                docs_sections.push(format!(
                    "### Doc Skill: {}\nPath: {}\n\n{}",
                    item.name, item.relative_path, sliced
                ));
            }
        }
    }

    if docs_sections.is_empty() {
        Ok(format!(
            "# Documentation Guidance for '{}' ({})\n\nNo matching documentation skills found in catalog for search term: '{}'.",
            id, query, search_term
        ))
    } else {
        Ok(format!(
            "# Documentation Guidance for '{}'\n\nQuery: '{}'\n\n{}",
            id,
            search_term,
            docs_sections.join("\n\n---\n\n")
        ))
    }
}

pub(crate) fn handle_workflow(arguments: &Value) -> Result<String, (i32, String)> {
    let stage = arguments
        .get("identifier")
        .or_else(|| arguments.get("topic"))
        .and_then(|i| i.as_str())
        .unwrap_or("plan")
        .trim()
        .to_lowercase();

    if stage.contains("..") || stage.contains('/') || stage.contains('\\') || stage.contains('\0') {
        return Err((
            -32602,
            format!(
                "Invalid workflow stage identifier '{}': path traversal characters are not allowed.",
                stage
            ),
        ));
    }

    let candidates = [
        format!("workflow-modes/references/workflow-{}.md", stage),
        format!("workflow-modes/references/{}.md", stage),
        format!("skills/{}/SKILL.md", stage),
    ];
    let content = candidates
        .iter()
        .find_map(|p| get_embedded_skill(p).map(|c| compress_markdown(&c)))
        .unwrap_or_else(|| {
            format!("# Dev Workflow Guidance: [{}]\n\nRecommended Flow: Context -> Plan -> Ask/Revise -> Build -> Test/Recheck -> Fix -> Document", stage)
        });
    Ok(content)
}

pub(crate) fn handle_ui_ux(query: &str) -> Result<String, (i32, String)> {
    let q = if query.is_empty() { "general" } else { query };
    Ok(format!(
        "# UI/UX Guidelines for '{}'\n\n- Styling: Modern CSS, Glassmorphism, Responsive Grid\n- Color Palette: Dark mode default, accessible HSL contrast ratios\n- Typography: Inter/Outfit readable hierarchy\n- Accessibility: Semantic HTML5, unique ARIA IDs",
        q
    ))
}
