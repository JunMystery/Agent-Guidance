use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessedQueryIntent {
    pub cleaned_query: String,
    pub slash_commands: Vec<String>,
    pub mentioned_skills: Vec<String>,
    pub is_empty_task: bool,
    pub is_question: bool,
}

pub fn analyze_query_intent(query: &str, known_skill_names: &[String]) -> ProcessedQueryIntent {
    let (cleaned_query, slash_commands) = strip_slash_commands(query);
    let is_empty_task = cleaned_query.trim().is_empty();
    let mentioned_skills = if !is_empty_task {
        detect_mentioned_skills(&cleaned_query, known_skill_names)
    } else {
        Vec::new()
    };
    let is_question = if !is_empty_task {
        is_informational_question(&cleaned_query)
    } else {
        false
    };

    ProcessedQueryIntent {
        cleaned_query,
        slash_commands,
        mentioned_skills,
        is_empty_task,
        is_question,
    }
}

pub fn strip_slash_commands(text: &str) -> (String, Vec<String>) {
    let mut remaining_words = Vec::new();
    let mut slash_commands = Vec::new();
    let mut seen = HashSet::new();

    for token in text.split_whitespace() {
        if let Some(rest) = token.strip_prefix('/') {
            let cmd_name = rest
                .trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
                .to_lowercase();
            if !cmd_name.is_empty() && seen.insert(cmd_name.clone()) {
                slash_commands.push(cmd_name);
            }
        } else {
            remaining_words.push(token);
        }
    }

    (remaining_words.join(" "), slash_commands)
}

fn normalize_identifier(name: &str) -> String {
    name.trim().to_lowercase().replace('-', "_")
}

pub fn detect_mentioned_skills(text: &str, known_skill_names: &[String]) -> Vec<String> {
    let mut matched = Vec::new();
    let mut seen = HashSet::new();

    const BUILTIN_IDE_SKILLS: &[&str] = &[
        "generative_ui",
        "automation",
        "plugin",
        "antigravity_guide",
        "agy_customizations",
        "ui_plugin_navigation",
        "migrate_workflows",
    ];

    let lower_text = text.to_lowercase();
    let normalized_text = lower_text.replace('-', "_");

    for known in known_skill_names {
        let norm_known = normalize_identifier(known);
        if norm_known.len() < 3 || seen.contains(known) {
            continue;
        }

        let is_found = if normalized_text.contains(&norm_known) {
            let pattern = format!(r"(?i)\b{}\b", regex::escape(&norm_known));
            regex::Regex::new(&pattern)
                .map(|re| re.is_match(&normalized_text))
                .unwrap_or(false)
        } else {
            false
        };

        if is_found && seen.insert(known.clone()) {
            matched.push(known.clone());
        }
    }

    for builtin in BUILTIN_IDE_SKILLS {
        let norm_known = normalize_identifier(builtin);
        if seen.contains(*builtin) {
            continue;
        }

        let is_found = if normalized_text.contains(&norm_known) {
            let pattern = format!(r"(?i)\b{}\b", regex::escape(&norm_known));
            regex::Regex::new(&pattern)
                .map(|re| re.is_match(&normalized_text))
                .unwrap_or(false)
        } else {
            false
        };

        if is_found && seen.insert(builtin.to_string()) {
            matched.push(builtin.to_string());
        }
    }

    matched
}

pub fn is_informational_question(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.ends_with('?') || trimmed.ends_with("?\"") || trimmed.ends_with("?'") {
        return true;
    }

    let lower = trimmed.to_lowercase();
    const QUESTION_PATTERNS: &[&str] = &[
        "how to", "how do", "how can", "how does", "what is", "what are",
        "why is", "why does", "where is", "when to", "who is", "which is",
        "can i", "is there", "could you", "explain", "compare", "review",
        "làm sao", "làm thế nào", "thế nào", "là gì", "tại sao", "ở đâu",
        "khi nào", "ai là", "có được không", "được không", "sao lại",
        "cho hỏi", "giải thích", "so sánh", "đánh giá", "phân tích"
    ];

    for pat in QUESTION_PATTERNS {
        if lower.starts_with(pat) || lower.contains(&format!(" {} ", pat)) {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_slash_commands() {
        let (cleaned, cmds) = strip_slash_commands("/generative_ui render 3 cards /plan");
        assert_eq!(cleaned, "render 3 cards");
        assert_eq!(cmds, vec!["generative_ui", "plan"]);

        let (empty_cleaned, empty_cmds) = strip_slash_commands("/generative_ui");
        assert_eq!(empty_cleaned, "");
        assert_eq!(empty_cmds, vec!["generative_ui"]);
    }

    #[test]
    fn test_is_informational_question() {
        assert!(is_informational_question("Làm thế nào để index skill?"));
        assert!(is_informational_question("how to run tests?"));
        assert!(is_informational_question("what is Clean Architecture"));
        assert!(is_informational_question("giải thích cho tôi file này"));
        assert!(!is_informational_question("tạo chức năng đăng nhập cho tôi"));
    }

    #[test]
    fn test_detect_mentioned_skills() {
        let known = vec!["agent-guidance".to_string(), "android-cli".to_string()];
        let found = detect_mentioned_skills("dùng android-cli để build", &known);
        assert_eq!(found, vec!["android-cli"]);

        let found_builtin = detect_mentioned_skills("sử dụng generative_ui", &known);
        assert_eq!(found_builtin, vec!["generative_ui"]);
    }

    #[test]
    fn test_analyze_query_intent() {
        let known = vec!["agent-guidance".to_string()];

        // Only slash command -> is_empty_task = true
        let res = analyze_query_intent("/generative_ui", &known);
        assert!(res.is_empty_task);
        assert_eq!(res.slash_commands, vec!["generative_ui"]);

        // Slash command + task
        let res = analyze_query_intent("/generative_ui render cards", &known);
        assert_eq!(res.cleaned_query, "render cards");
        assert_eq!(res.slash_commands, vec!["generative_ui"]);
        assert!(!res.is_empty_task);

        // Mentioned skill in text
        let res = analyze_query_intent("Dùng agent-guidance để audit", &known);
        assert_eq!(res.mentioned_skills, vec!["agent-guidance"]);
        assert!(!res.is_question);

        // Question without skill
        let res = analyze_query_intent("Làm thế nào để index dữ liệu?", &known);
        assert!(res.is_question);
        assert!(res.mentioned_skills.is_empty());
    }
}
