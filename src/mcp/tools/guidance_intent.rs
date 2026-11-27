use std::collections::HashSet;
use crate::catalog::language_detector::contains_word;

const KNOWN_IDE_COMMANDS: &[&str] = &[
    "generative_ui",
    "plan",
    "goal",
    "compact",
    "schedule",
    "grill_me",
    "learn",
    "automation",
    "plugin",
    "swarm",
    "mcp",
    "help",
    "status",
    "clear",
    "reset",
    "review",
    "test",
    "fix",
    "build",
    "refactor",
    "debug",
    "audit",
    "verify",
    "init",
    "docs",
    "precode",
    "ui_ux",
    "antigravity_guide",
    "agy_customizations",
    "ui_plugin_navigation",
    "migrate_workflows",
    "migrate_agent_settings",
    "run_workflow",
    "conversations",
    "swarm_monitor",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessedQueryIntent {
    pub cleaned_query: String,
    pub slash_commands: Vec<String>,
    pub mentioned_skills: Vec<String>,
    pub is_empty_task: bool,
    pub is_question: bool,
}

pub fn analyze_query_intent(query: &str, known_skill_names: &[String]) -> ProcessedQueryIntent {
    let (cleaned_query, slash_commands, slash_skills) =
        strip_slash_commands_with_skills(query, known_skill_names);
    let is_empty_task = cleaned_query.trim().is_empty();
    let mut mentioned_skills = Vec::new();
    let mut seen = HashSet::new();
    for s in slash_skills {
        if seen.insert(s.clone()) {
            mentioned_skills.push(s);
        }
    }
    if !is_empty_task {
        for s in detect_mentioned_skills(&cleaned_query, known_skill_names) {
            if seen.insert(s.clone()) {
                mentioned_skills.push(s);
            }
        }
    }
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

#[cfg(test)]
pub fn strip_slash_commands(text: &str) -> (String, Vec<String>) {
    let (cleaned, cmds, _) = strip_slash_commands_with_skills(text, &[]);
    (cleaned, cmds)
}

fn strip_slash_commands_with_skills(
    text: &str,
    known_skill_names: &[String],
) -> (String, Vec<String>, Vec<String>) {
    let mut remaining_words = Vec::new();
    let mut slash_commands = Vec::new();
    let mut slash_skills = Vec::new();
    let mut seen_cmds = HashSet::new();
    let mut seen_skills = HashSet::new();

    for token in text.split_whitespace() {
        if let Some(rest) = token.strip_prefix('/') {
            let trimmed_rest =
                rest.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '-');
            let is_path_like = trimmed_rest.contains('/')
                || trimmed_rest.contains('\\')
                || trimmed_rest.contains('.');
            if !is_path_like {
                let raw_cmd = trimmed_rest
                    .trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '-');
                let norm_cmd = normalize_identifier(raw_cmd);
                if !norm_cmd.is_empty() {
                    if KNOWN_IDE_COMMANDS.contains(&norm_cmd.as_str()) {
                        if seen_cmds.insert(norm_cmd.clone()) {
                            slash_commands.push(norm_cmd);
                        }
                        continue;
                    }
                    if let Some(matched_skill) = known_skill_names
                        .iter()
                        .find(|s| normalize_identifier(s) == norm_cmd)
                    {
                        if seen_skills.insert(matched_skill.clone()) {
                            slash_skills.push(matched_skill.clone());
                        }
                        remaining_words.push(raw_cmd);
                        continue;
                    }
                }
            }
        }
        remaining_words.push(token);
    }

    (remaining_words.join(" "), slash_commands, slash_skills)
}

pub fn is_tooling_skill(skill_name: &str) -> bool {
    let s = skill_name.to_lowercase();
    const TOOLING_KEYWORDS: &[&str] = &[
        "git",
        "github",
        "docker",
        "ci",
        "cd",
        "bash",
        "shell",
        "powershell",
        "deploy",
        "deployment",
        "release",
        "docs",
    ];
    TOOLING_KEYWORDS.iter().any(|kw| contains_word(&s, kw))
}

fn normalize_identifier(name: &str) -> String {
    name.trim().to_lowercase().replace('-', "_")
}

fn find_identifier_word_pos(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    let is_ident_char = |c: char| c.is_alphanumeric() || c == '_';
    for (idx, _) in haystack.match_indices(needle) {
        let before_ok = haystack[..idx]
            .chars()
            .next_back()
            .map(|c| !is_ident_char(c))
            .unwrap_or(true);
        let after_ok = haystack[idx + needle.len()..]
            .chars()
            .next()
            .map(|c| !is_ident_char(c))
            .unwrap_or(true);
        if before_ok && after_ok {
            return Some(idx);
        }
    }
    None
}

pub fn detect_mentioned_skills(text: &str, known_skill_names: &[String]) -> Vec<String> {
    let mut matched: Vec<(usize, String)> = Vec::new();
    let mut seen = HashSet::new();

    const BUILTIN_IDE_SKILLS: &[&str] = &[
        "generative_ui",
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
        if let Some(pos) = find_identifier_word_pos(&normalized_text, &norm_known) {
            if seen.insert(known.clone()) {
                matched.push((pos, known.clone()));
            }
        }
    }

    for builtin in BUILTIN_IDE_SKILLS {
        let norm_known = normalize_identifier(builtin);
        if seen.contains(*builtin) {
            continue;
        }
        if let Some(pos) = find_identifier_word_pos(&normalized_text, &norm_known) {
            if seen.insert(builtin.to_string()) {
                matched.push((pos, builtin.to_string()));
            }
        }
    }

    matched.sort_by_key(|(pos, _)| *pos);
    matched.into_iter().map(|(_, name)| name).collect()
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
        "can i", "is there", "explain", "compare",
        "làm sao", "làm thế nào", "thế nào", "là gì", "tại sao", "ở đâu",
        "khi nào", "ai là", "có được không", "được không", "sao lại",
        "cho hỏi", "giải thích", "so sánh"
    ];

    for pat in QUESTION_PATTERNS {
        if contains_word(&lower, pat) {
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

        // Unix paths and REST routes must NOT be stripped as slash commands
        let (route_cleaned, route_cmds) = strip_slash_commands("/plan fix /api/v1/users and /etc/nginx.conf");
        assert_eq!(route_cleaned, "fix /api/v1/users and /etc/nginx.conf");
        assert_eq!(route_cmds, vec!["plan"]);
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

    #[test]
    fn test_slash_command_hardening_and_hyphen_normalization() {
        // Hyphenated /generative-ui normalizes to generative_ui
        let res = analyze_query_intent("/generative-ui render dashboard", &[]);
        assert_eq!(res.slash_commands, vec!["generative_ui"]);
        assert_eq!(res.cleaned_query, "render dashboard");

        // Unknown /health, /login, /tmp are NOT stripped
        let (cleaned, cmds) = strip_slash_commands("Check /health and /login in /tmp");
        assert_eq!(cleaned, "Check /health and /login in /tmp");
        assert!(cmds.is_empty());

        // Known /<skill> is preserved in cleaned_query and boosted in mentioned_skills
        let known = vec!["sql-safety".to_string(), "agent-guidance".to_string()];
        let skill_res = analyze_query_intent("/sql-safety fix query", &known);
        assert_eq!(skill_res.cleaned_query, "sql-safety fix query");
        assert_eq!(skill_res.mentioned_skills, vec!["sql-safety"]);
    }

    #[test]
    fn test_word_boundaries_question_patterns_and_mention_order() {
        assert!(!is_tooling_skill("pricing-rules"));
        assert!(!is_tooling_skill("incident-response"));
        assert!(is_tooling_skill("ci-cd-and-automation"));
        assert!(is_tooling_skill("git-ops"));

        assert!(!is_informational_question("Could you implement JWT auth"));
        assert!(!is_informational_question("Review PR security"));
        assert!(!is_informational_question("Đánh giá hiệu năng API"));
        assert!(!is_informational_question("Phân tích bug crash"));

        // Generic plugin/automation without slash prefix must not auto-inject
        assert!(detect_mentioned_skills("write automation tests with plugin", &[]).is_empty());

        // Order of multiple mentioned skills preserved
        let known = vec!["agent-guidance".to_string(), "sql-safety".to_string()];
        assert_eq!(
            detect_mentioned_skills("use sql-safety then agent-guidance", &known),
            vec!["sql-safety", "agent-guidance"]
        );
    }
}
