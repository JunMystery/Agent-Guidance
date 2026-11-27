use crate::catalog::language_detector::contains_word;
use super::helpers::truncate_chars;

pub(crate) fn format_search_output(
    query: &str,
    context_note: Option<&str>,
    formatted_results: &[String],
    options: &[String],
    engine_name: &str,
) -> Result<String, (i32, String)> {
    let results_body = if formatted_results.is_empty() {
        "No matching skills found.".to_string()
    } else {
        format!("Recommended Skills:\n{}", formatted_results.join("\n"))
    };

    let context_section = match context_note {
        Some(note) if !note.trim().is_empty() => format!("\nContext Source: {}", note.trim()),
        _ => String::new(),
    };

    let q_lower = query.to_lowercase();
    let gen_ui_note = if contains_word(&q_lower, "ui")
        || contains_word(&q_lower, "widget")
        || options.iter().any(|o| o.contains("generative_ui"))
    {
        "\n\n> [!IMPORTANT] **Generative UI Protocol**: Create HTML artifact(s) (`UserFacing: true`) and `<agent-embed>` BEFORE asking layout choices."
    } else {
        ""
    };

    let next_step_prompt = if formatted_results.is_empty() {
        format!(
            "-> No matching skills found. Proceed to planning or `project_context`.{}",
            gen_ui_note
        )
    } else {
        format!(
            "-> SKILL_PROPOSAL: MANDATORY USER INTERACTION REQUIRED. Call `ask_question` (`is_multi_select: true`) with the options above translated to the user's language (full diacritics, `[(Recommended) ]{{name}} - {{desc <= 50c}}`), then `select_skills(skills=[...], user_confirmed=true)` (or `select_skills(skills=[], user_confirmed=true)` if none).{}",
            gen_ui_note
        )
    };

    Ok(format!(
        "# Skill Search Results for '{}' [{}]{}\nMatches Found: {}\n\n{}\n\n{}",
        query, engine_name, context_section, formatted_results.len(), results_body, next_step_prompt
    ))
}

/// Extracts and formats the `description` field from YAML frontmatter bounded by `---`.
pub(crate) fn extract_description(content: &str) -> Option<String> {
    let clean = content.strip_prefix('\u{feff}').unwrap_or(content);
    let mut lines = clean.lines();
    let first = lines.find(|l| !l.trim().is_empty())?;
    if first.trim() != "---" {
        return None;
    }

    let mut raw_desc: Option<String> = None;
    let mut collecting_block = false;
    let mut block_parts: Vec<String> = Vec::new();

    for line in lines {
        let t = line.trim();
        if t == "---" || t == "..." {
            break;
        }
        if collecting_block {
            if line.starts_with([' ', '\t']) {
                if !t.is_empty() {
                    block_parts.push(t.to_string());
                }
                continue;
            } else {
                collecting_block = false;
                if !block_parts.is_empty() {
                    raw_desc = Some(block_parts.join(" "));
                    break;
                }
            }
        }

        if !line.starts_with([' ', '\t']) {
            if let Some(rest) = t.strip_prefix("description:") {
                let val = rest.trim();
                if matches!(val, ">" | "|" | ">-" | "|-" | "") {
                    collecting_block = true;
                } else {
                    raw_desc = Some(val.to_string());
                    break;
                }
            }
        }
    }

    if raw_desc.is_none() && !block_parts.is_empty() {
        raw_desc = Some(block_parts.join(" "));
    }

    let val = raw_desc?;
    let cleaned = val
        .trim()
        .trim_matches(|c| matches!(c, '"' | '\'' | '`'))
        .trim();
    if cleaned.is_empty() || matches!(cleaned, ">" | "|" | ">-" | "|-") {
        return None;
    }
    let normalized = cleaned.replace(['\u{2014}', '\u{2013}'], "-");
    let first_sentence = normalized.split(". ").next().unwrap_or(&normalized).trim();
    let short = if first_sentence.chars().count() > 50 {
        let truncated = truncate_chars(first_sentence, 47).trim_end();
        format!("{}...", truncated)
    } else {
        first_sentence.to_string()
    };
    Some(short)
}
