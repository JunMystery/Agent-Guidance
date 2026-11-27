use serde_json::Value;

use crate::catalog::language_detector::contains_word;
use crate::catalog::store::{SkillSource, load_all_skills};
use crate::config::current_config;
use crate::mcp::state::ServerState;
use super::guidance_format::{extract_description, format_search_output};
use super::guidance_intent::{analyze_query_intent, is_tooling_skill};
use super::helpers::{detect_project_path, ensure_not_cancelled, truncate_chars};

pub(crate) fn handle_list(arguments: &Value, state: &mut ServerState) -> Result<String, (i32, String)> {
    let proj_path_arg = arguments
        .get("project_path")
        .and_then(|p| p.as_str())
        .unwrap_or(".");
    let proj_path = detect_project_path(proj_path_arg, state);
    let topic_filter = arguments
        .get("topic")
        .or_else(|| arguments.get("query"))
        .or_else(|| arguments.get("filter"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase());
    let limit = arguments
        .get("limit")
        .or_else(|| arguments.get("max_results"))
        .and_then(|v| v.as_u64())
        .map(|n| (n as usize).clamp(1, 500))
        .unwrap_or(50);

    let skills = load_all_skills(&proj_path);
    let filtered: Vec<_> = skills
        .iter()
        .filter(|s| match &topic_filter {
            Some(topic) => {
                s.name.to_lowercase().contains(topic)
                    || s.relative_path.to_lowercase().contains(topic)
            }
            None => true,
        })
        .collect();
    let total_matching = filtered.len();
    let names: Vec<String> = filtered
        .into_iter()
        .take(limit)
        .map(|s| format!("- {} ({})", s.name, s.relative_path))
        .collect();
    let header_note = if names.len() < total_matching {
        format!(
            "showing {} of {}; pass `limit` or `topic` to filter",
            names.len(),
            total_matching
        )
    } else {
        total_matching.to_string()
    };
    Ok(format!(
        "# Registered Skills Catalog ({})\n\n{}",
        header_note,
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
    let max_results = arguments
        .get("max_results")
        .and_then(|v| v.as_u64())
        .map(|n| (n as usize).clamp(1, 20))
        .unwrap_or(6);

    let primary_prompt = if !query.trim().is_empty() {
        query.trim()
    } else if !task_arg.trim().is_empty() {
        task_arg.trim()
    } else if let Some(active) = &state.active_task {
        active.as_str()
    } else if let Some(intent) = &state.user_intent_summary {
        intent.as_str()
    } else {
        "general programming guidance"
    };

    // Collect recent task and work context (Multi-tier context)
    let mut recent_context_items = Vec::new();
    if let Some(active) = &state.active_task {
        if !active.trim().eq_ignore_ascii_case(primary_prompt) {
            recent_context_items.push(format!("Active: {}", active));
        }
    }
    for t in &state.recent_tasks {
        let t_lower = t.to_lowercase();
        if !t.trim().eq_ignore_ascii_case(primary_prompt)
            && !recent_context_items
                .iter()
                .any(|item| item.to_lowercase().contains(&t_lower))
        {
            recent_context_items.push(t.clone());
        }
    }
    if !state.modified_files.is_empty() {
        let files_preview: Vec<_> = state.modified_files.iter().take(3).cloned().collect();
        recent_context_items.push(format!("Files: {}", files_preview.join(", ")));
    }

    let context_note_str = if !recent_context_items.is_empty() {
        format!("[{}]", recent_context_items.join(" | "))
    } else {
        String::new()
    };
    let context_note = if context_note_str.is_empty() {
        None
    } else {
        Some(context_note_str.as_str())
    };

    let all_skills = load_all_skills(&proj_path);
    let skill_names: Vec<String> = all_skills.iter().map(|s| s.name.clone()).collect();

    // Analyze intent strictly on primary_prompt BEFORE appending files/tech_stack
    let intent = analyze_query_intent(primary_prompt, &skill_names);
    let norm_prompt = primary_prompt.to_lowercase().replace('-', "_");
    let is_gen_ui = intent.slash_commands.iter().any(|c| c == "generative_ui")
        || intent.mentioned_skills.iter().any(|s| s == "generative_ui")
        || contains_word(&norm_prompt, "generative_ui");

    if intent.is_empty_task {
        state.pending_skill_proposals.clear();
        let cmds_joined = intent.slash_commands.iter().map(|c| format!("/{}", c)).collect::<Vec<_>>().join(", ");
        let gen_ui_note = if is_gen_ui {
            "\n\n-> [STRICT MANDATE] Calling `ask_question` is STRICTLY PROHIBITED in this conversation.\nDo NOT show skill selection dialogs, do NOT ask for layout choices, and do NOT interview the user.\nImmediately generate self-contained HTML artifact(s) via `write_file` (`UserFacing: true`) and embed inline via `<agent-embed src=\"file:///.../widget.html\"></agent-embed>`."
        } else {
            ""
        };
        return Ok(format!(
            "# Slash Command Detected: {}\n\n-> Bypassing skill proposals. Proceed directly to execute the slash command workflow (no skill confirmation needed).{}",
            if cmds_joined.is_empty() { "Command".to_string() } else { cmds_joined },
            gen_ui_note
        ));
    }

    if is_gen_ui {
        state.pending_skill_proposals.clear();
        return Ok(format!(
            "# Generative UI Active: {}\n\n-> [STRICT MANDATE] Calling `ask_question` is STRICTLY PROHIBITED in this conversation.\nDo NOT show skill selection dialogs, do NOT ask for layout choices, and do NOT interview the user.\nImmediately generate self-contained HTML artifact(s) via `write_file` (`UserFacing: true`) and embed inline via `<agent-embed src=\"file:///.../widget.html\"></agent-embed>`.",
            intent.cleaned_query
        ));
    }

    if intent.is_question && intent.mentioned_skills.is_empty() {
        state.pending_skill_proposals.clear();
        return Ok(format!(
            "# Informational Query Detected for '{}'\n\n-> Bypassing skill proposals. Answer the question directly without injecting skills or calling ask_question.",
            intent.cleaned_query
        ));
    }

    let mut search_parts = vec![intent.cleaned_query.clone()];
    let cleaned_lower = intent.cleaned_query.to_lowercase();
    if !workflow_arg.trim().is_empty() && !cleaned_lower.contains(&workflow_arg.trim().to_lowercase()) {
        search_parts.push(workflow_arg.trim().to_string());
    }
    if !tech_stack_arg.trim().is_empty() && !cleaned_lower.contains(&tech_stack_arg.trim().to_lowercase()) {
        search_parts.push(tech_stack_arg.trim().to_string());
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
            search_parts.push(f_str);
        }
    }
    let search_query = search_parts.join(" ");

    let cfg = current_config();
    if cfg.server.is_remote() {
        let client = crate::client::default_client();
        match client.search_skills(&search_query, max_results) {
            Ok(remote_res) => {
                let mut seen_remote = std::collections::HashSet::new();
                let mut mentioned_bucket = Vec::new();
                let mut domain_bucket = Vec::new();
                let mut tooling_bucket = Vec::new();

                for item in remote_res.skills {
                    let is_mentioned = intent.mentioned_skills.contains(&item.name);
                    if (!is_mentioned && item.score < 0.65) || !seen_remote.insert(item.name.clone()) {
                        continue;
                    }
                    if let Some(m_idx) = intent.mentioned_skills.iter().position(|m| m == &item.name) {
                        mentioned_bucket.push((m_idx, item));
                    } else if is_tooling_skill(&item.name) {
                        tooling_bucket.push(item);
                    } else {
                        domain_bucket.push(item);
                    }
                }
                mentioned_bucket.sort_by_key(|(m_idx, _)| *m_idx);
                let mut tiered_remote: Vec<_> = mentioned_bucket.into_iter().map(|(_, item)| item).collect();
                tiered_remote.extend(domain_bucket);
                tiered_remote.extend(tooling_bucket);
                tiered_remote.truncate(max_results);

                let mut options = Vec::new();
                let mut proposals = Vec::new();
                let mut formatted = Vec::new();

                for (idx, item) in tiered_remote.into_iter().enumerate() {
                    let desc = if !item.summary.is_empty() {
                        item.summary
                    } else if !item.intent.is_empty() {
                        item.intent
                    } else {
                        item.title
                    };
                    let short_desc = if desc.chars().count() > 50 {
                        let t = truncate_chars(&desc, 47).trim_end().to_string();
                        format!("{}...", t)
                    } else {
                        desc
                    };
                    let is_mentioned = intent.mentioned_skills.contains(&item.name);
                    let prefix = if is_mentioned || idx == 0 { "(Recommended) " } else { "" };
                    let opt_label = if short_desc.is_empty() {
                        format!("{}{}", prefix, item.name)
                    } else {
                        format!("{}{} - {}", prefix, item.name, short_desc)
                    };
                    options.push(opt_label.clone());
                    proposals.push((item.name.clone(), format!("remote://skills/{}", item.name), item.score));
                    formatted.push(format!("- {} [Remote, {:.2}]", opt_label, item.score));
                }

                state.pending_skill_proposals = proposals;
                return format_search_output(&search_query, context_note, &formatted, &options, "Remote ML Worker (In-Memory AGV1)");
            }
            Err(e) => {
                tracing::warn!("Remote skill search failed: {}; attempting fallback", e);
            }
        }
    }

    let (ranked_results, _profile) = super::guidance_search_stages::discover_skills_two_stage(
        &proj_path,
        &search_query,
        &all_skills,
        &intent.mentioned_skills,
    );
    ensure_not_cancelled(state)?;

    let tiered_results = super::guidance_search_stages::tier_skill_results(
        ranked_results,
        &intent.mentioned_skills,
        max_results,
    );

    state.pending_skill_proposals = tiered_results
        .iter()
        .map(|(score, item)| {
            (item.name.clone(), item.relative_path.clone(), *score)
        })
        .collect();

    let options: Vec<String> = tiered_results
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
                format!("{}{} - {}", prefix, item.name, short_desc)
            }
        })
        .collect();

    let formatted_results: Vec<String> = tiered_results
        .into_iter()
        .enumerate()
        .map(|(idx, (score, item))| {
            let opt = options.get(idx).map(String::as_str).unwrap_or(&item.name);
            match &item.source {
                SkillSource::Embedded => format!("- {} [Embedded, {:.2}]", opt, score),
                SkillSource::LocalWorkspace(path) => format!("- {} [Local: {}, {:.2}]", opt, path, score),
            }
        })
        .collect();

    format_search_output(&search_query, context_note, &formatted_results, &options, "Two-Stage Tiered Hybrid Vector & Cross-Encoder")
}
