use serde_json::Value;

use crate::catalog::language_detector::detect_language_fast;
use crate::catalog::store::{SkillSource, load_all_skills};
use crate::config::current_config;
use crate::mcp::state::ServerState;
use crate::ml::embeddings::hybrid_vector_search;
use crate::ml::llm_selector::LLMSelector;
use super::guidance_format::{extract_description, format_search_output};
use super::guidance_intent::analyze_query_intent;
use super::helpers::{detect_project_path, ensure_not_cancelled, truncate_chars};

pub(crate) fn handle_list(arguments: &Value, state: &mut ServerState) -> Result<String, (i32, String)> {
    let proj_path_arg = arguments
        .get("project_path")
        .and_then(|p| p.as_str())
        .unwrap_or(".");
    let proj_path = detect_project_path(proj_path_arg, state);
    let skills = load_all_skills(&proj_path);
    let names: Vec<String> = skills
        .iter()
        .map(|s| format!("- {} ({})", s.name, s.relative_path))
        .collect();
    Ok(format!(
        "# Registered Skills Catalog ({})\n\n{}",
        names.len(),
        names.join("\n")
    ))
}

pub(crate) fn handle_search(
    arguments: &Value,
    query: &str,
    state: &mut ServerState,
) -> Result<String, (i32, String)> {
    let proj_path_arg = arguments
        .get("project_path")
        .and_then(|p| p.as_str())
        .unwrap_or(".");
    let proj_path = detect_project_path(proj_path_arg, state);

    ensure_not_cancelled(state)?;

    let task_arg = arguments.get("task").and_then(|v| v.as_str()).unwrap_or("");
    let workflow_arg = arguments.get("workflow").and_then(|v| v.as_str()).unwrap_or("");
    let tech_stack_arg = arguments.get("tech_stack").and_then(|v| v.as_str()).unwrap_or("");
    let files_arg = arguments.get("files").or_else(|| arguments.get("related_files"));

    let mut query_parts = Vec::new();
    if !query.trim().is_empty() {
        query_parts.push(query.trim().to_string());
    }
    if !task_arg.trim().is_empty() && !query.contains(task_arg) {
        query_parts.push(task_arg.trim().to_string());
    }
    if !workflow_arg.trim().is_empty() && !query.contains(workflow_arg) {
        query_parts.push(workflow_arg.trim().to_string());
    }
    if !tech_stack_arg.trim().is_empty() && !query.contains(tech_stack_arg) {
        query_parts.push(tech_stack_arg.trim().to_string());
    }
    if let Some(files) = files_arg {
        let f_str = if let Some(arr) = files.as_array() {
            arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(" ")
        } else if let Some(s) = files.as_str() {
            s.to_string()
        } else {
            String::new()
        };
        if !f_str.is_empty() {
            query_parts.push(f_str);
        }
    }
    if query_parts.is_empty() {
        if let Some(intent) = &state.user_intent_summary {
            query_parts.push(intent.clone());
        }
    }
    if query_parts.is_empty() && !state.modified_files.is_empty() {
        query_parts.push(state.modified_files.join(" "));
    }

    let all_skills = load_all_skills(&proj_path);
    let skill_names: Vec<String> = all_skills.iter().map(|s| s.name.clone()).collect();

    let raw_search_query = if query_parts.is_empty() {
        "general programming guidance".to_string()
    } else {
        query_parts.join(" ")
    };

    let intent = analyze_query_intent(&raw_search_query, &skill_names);
    let is_gen_ui = intent.slash_commands.iter().any(|c| c == "generative_ui")
        || intent.mentioned_skills.iter().any(|s| s == "generative_ui")
        || raw_search_query.to_lowercase().contains("generative_ui");

    if intent.is_empty_task {
        state.pending_skill_proposals.clear();
        let cmds_joined = intent.slash_commands.iter().map(|c| format!("/{}", c)).collect::<Vec<_>>().join(", ");
        let gen_ui_note = if is_gen_ui {
            "\n\n> [!IMPORTANT] **Generative UI Protocol**: When designing UI alternatives, create visual HTML artifact file(s) via `write_file` (with `UserFacing: true`) and inline embed via `<agent-embed src=\"file:///.../widget.html\"></agent-embed>` BEFORE soliciting user layout choices."
        } else {
            ""
        };
        return Ok(format!(
            "# Slash Command Detected: {}\n\n-> Bypassing skill proposals. Proceed directly to execute the slash command workflow (no skill confirmation needed).{}",
            if cmds_joined.is_empty() { "Command".to_string() } else { cmds_joined },
            gen_ui_note
        ));
    }

    if intent.is_question && intent.mentioned_skills.is_empty() {
        state.pending_skill_proposals.clear();
        return Ok(format!(
            "# Informational Query Detected for '{}'\n\n-> Bypassing skill proposals. Answer the question directly without injecting skills or calling ask_question.",
            intent.cleaned_query
        ));
    }

    let search_query = intent.cleaned_query;

    let cfg = current_config();
    if cfg.server.is_remote() {
        let client = crate::client::default_client();
        match client.search_skills(&search_query, 6) {
            Ok(remote_res) => {
                let mut options = Vec::new();
                let mut proposals = Vec::new();
                let mut formatted = Vec::new();

                for item in remote_res.skills {
                    let desc = if !item.summary.is_empty() {
                        item.summary
                    } else if !item.intent.is_empty() {
                        item.intent
                    } else {
                        item.title
                    };
                    let short_desc = if desc.chars().count() > 38 {
                        let t = truncate_chars(&desc, 35).trim_end().to_string();
                        format!("{}...", t)
                    } else {
                        desc
                    };
                    let opt_label = if intent.mentioned_skills.contains(&item.name) {
                        format!("(Recommended) {} - {}", item.name, short_desc)
                    } else {
                        format!("{} - {}", item.name, short_desc)
                    };
                    options.push(opt_label);
                    proposals.push((item.name.clone(), format!("remote://skills/{}", item.name), item.score));
                    formatted.push(format!(
                        "- {} [Remote ML Worker] (Score: {:.2})\n  Path: skills/{}/SKILL.md",
                        item.name, item.score, item.name
                    ));
                }

                state.pending_skill_proposals = proposals;
                return format_search_output(&search_query, &formatted, &options, "Remote ML Worker (In-Memory AGV1)", is_gen_ui);
            }
            Err(e) => {
                tracing::warn!("Remote skill search failed: {}; attempting fallback", e);
            }
        }
    }

    let profile = detect_language_fast(&proj_path, &search_query);

    let stage1_results = hybrid_vector_search(&search_query, &all_skills, 20);
    ensure_not_cancelled(state)?;

    let stage1_5_results = crate::ml::skill_graphrag_gate::apply_graphrag_context_gating(
        &proj_path,
        &search_query,
        stage1_results,
        20,
    );
    ensure_not_cancelled(state)?;

    let selector = LLMSelector::new();
    let reranked = selector.rerank(&search_query, stage1_5_results, &profile, 20);
    ensure_not_cancelled(state)?;

    let final_results = crate::ml::skill_analytics::apply_analytics_boost(reranked, &proj_path);

    let mut seen_names = std::collections::HashSet::new();
    let mut deduped_results = Vec::new();

    // 1. Prioritize explicitly mentioned skills to the front with score 1.0
    for mentioned in &intent.mentioned_skills {
        if let Some(item) = all_skills.iter().find(|s| s.name == *mentioned) {
            if seen_names.insert(item.name.clone()) {
                deduped_results.push((1.0, item.clone()));
            }
        }
    }

    // 2. Add algorithmic results
    for (score, item) in final_results {
        if score < 0.65 {
            continue;
        }
        if seen_names.insert(item.name.clone()) {
            deduped_results.push((score, item));
            if deduped_results.len() >= 6 {
                break;
            }
        }
    }

    state.pending_skill_proposals = deduped_results
        .iter()
        .map(|(score, item)| {
            (item.name.clone(), item.relative_path.clone(), *score)
        })
        .collect();

    let options: Vec<String> = deduped_results
        .iter()
        .enumerate()
        .map(|(idx, (_score, item))| {
            let short_desc = extract_description(&item.content)
                .unwrap_or_else(|| "Skill guidance".to_string());
            let is_mentioned = intent.mentioned_skills.contains(&item.name);
            let prefix = if is_mentioned || idx == 0 { "(Recommended) " } else { "" };
            if short_desc.is_empty() {
                format!("{}{}", prefix, item.name)
            } else {
                format!("{}{}{}", prefix, item.name, format!(" - {}", short_desc))
            }
        })
        .collect();

    let formatted_results: Vec<String> = deduped_results
        .into_iter()
        .map(|(score, item)| {
            let source_tag = match &item.source {
                SkillSource::Embedded => "[Embedded]".to_string(),
                SkillSource::LocalWorkspace(path) => format!("[Local Workspace: {}]", path),
            };
            format!(
                "- {} {} (Score: {:.2})\n  Path: {}",
                item.name, source_tag, score, item.relative_path
            )
        })
        .collect();

    format_search_output(&search_query, &formatted_results, &options, "3-Stage Hybrid Vector & Cross-Encoder", is_gen_ui)
}
