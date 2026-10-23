use super::helpers::truncate_chars;

pub(crate) fn format_search_output(
    query: &str,
    context_note: Option<&str>,
    formatted_results: &[String],
    options: &[String],
    engine_name: &str,
    is_gen_ui_active: bool,
) -> Result<String, (i32, String)> {
    let ask_question_example = serde_json::json!({
        "questions": [
            {
                "question": "Which skills would you like to inject for this task?",
                "options": options,
                "is_multi_select": true
            }
        ]
    });

    let gen_ui_note = if is_gen_ui_active
        || query.to_lowercase().contains("ui")
        || query.to_lowercase().contains("widget")
        || options.iter().any(|o| o.contains("generative_ui"))
    {
        "\n\n> [!IMPORTANT] **Generative UI Protocol**: When designing UI alternatives, create visual HTML artifact file(s) via `write_file` (with `UserFacing: true`) and inline embed via `<agent-embed src=\"file:///.../widget.html\"></agent-embed>` BEFORE soliciting user layout choices."
    } else {
        ""
    };

    let next_step_prompt = if formatted_results.is_empty() {
        format!(
            "-> No matching skills found. Proceed directly to task planning or codebase inspection (no skills need to be selected).{}",
            gen_ui_note
        )
    } else {
        format!(
            "-> SKILL_PROPOSAL: MANDATORY USER INTERACTION REQUIRED. Do NOT call `select_skills` automatically.\nYou MUST trigger the IDE tool `ask_question` with `is_multi_select: true` so the user selects which skills to inject.\n\nCRITICAL MULTILINGUAL MANDATE:\n1. Match User Language: Translate the question and each skill's description into the active conversational language used by the user in this chat (e.g. Vietnamese, Japanese, French, English, etc.).\n2. Accents & Diacritics: For languages with diacritics (like Vietnamese), MUST write natural accented text with full diacritics ('tiếng Việt có dấu đầy đủ'). NEVER strip accents or emit unaccented text ('tiếng Việt không dấu').\n3. Option Format: Format each option as `[(Recommended) ]{{name}} - {{translated_short_desc}}` (keep `{{name}}` exact, `{{translated_short_desc}}` <= 50 chars).\n\nSuggested template (translate question & descriptions before calling):\n```json\nask_question({})\n```\nAfter the user responds, call `select_skills(skills=[...], user_confirmed=true)` (or `select_skills(skills=[])` if none selected).{}",
            serde_json::to_string_pretty(&ask_question_example).unwrap_or_default(),
            gen_ui_note
        )
    };

    let results_body = if formatted_results.is_empty() {
        "No matching skills found for this task.".to_string()
    } else {
        format!("Recommended Skills:\n{}", formatted_results.join("\n"))
    };

    let context_section = match context_note {
        Some(note) if !note.trim().is_empty() => format!("\nContext Source: {}\n", note.trim()),
        _ => String::new(),
    };

    Ok(format!(
        "# Skill Search Results for '{}' [{}]\n{}Matches Found: {}\n\n{}\n\n{}",
        query, engine_name, context_section, formatted_results.len(), results_body, next_step_prompt
    ))
}

pub(crate) fn extract_description(content: &str) -> Option<String> {
    for line in content.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("description:") {
            let val = rest.trim().trim_matches(|c| matches!(c, '"' | '\'' | '`')).trim();
            if !val.is_empty() && val != ">" && val != "|" {
                let normalized = val.replace(['\u{2014}', '\u{2013}'], "-");
                let first = normalized.split(". ").next().unwrap_or(&normalized).trim();
                let short = if first.chars().count() > 50 {
                    let truncated = truncate_chars(first, 47).trim_end();
                    format!("{}...", truncated)
                } else {
                    first.to_string()
                };
                return Some(short);
            }
        }
    }
    None
}
