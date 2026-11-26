//! Query search intent classification and path role tagging.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchIntent {
    Logic,
    Guidance,
    Universal,
}

impl SearchIntent {
    pub fn from_str_opt(s: Option<&str>) -> Self {
        match s.map(|v| v.to_lowercase()).as_deref() {
            Some("logic") | Some("code") => SearchIntent::Logic,
            Some("guidance") | Some("docs") | Some("doc") => SearchIntent::Guidance,
            _ => SearchIntent::Universal,
        }
    }
}

pub fn classify_intent(query: &str) -> SearchIntent {
    let q = query.trim();
    if q.is_empty() {
        return SearchIntent::Universal;
    }

    let q_lower = q.to_lowercase();

    // Guidance markers: strong phrases (3 points each)
    let strong_guidance_phrases = [
        "how to", "how do", "what is", "best practice", "guidance",
        "guide", "docs", "documentation", "rule", "rules", "standard",
        "standards", "protocol", "protocols", "policy", "policies",
        "roadmap", "walkthrough", "convention", "conventions", "workflow gate",
    ];
    let strong_guidance_score = strong_guidance_phrases
        .iter()
        .filter(|&&phrase| q_lower.contains(phrase))
        .count() * 3;

    let guidance_exts = [".md", ".markdown", ".txt", ".yaml", ".yml", ".toml"];
    let has_guidance_ext = guidance_exts.iter().any(|&ext| q_lower.contains(ext));

    // Logic / Code markers
    let code_syntax_markers = ["::", "->", "()", "=>", "!=", "==", "<", ">"];
    let has_code_syntax = code_syntax_markers.iter().any(|&marker| q.contains(marker));

    let code_keywords = [
        "fn ", "pub ", "struct ", "impl ", "trait ", "class ", "interface ",
        "def ", "async ", "import ", "package ", "select ", "insert ", "update ",
        "delete ", "endpoint", "api/", "route",
    ];
    let logic_kw_score = code_keywords
        .iter()
        .filter(|&&kw| q_lower.contains(kw))
        .count() * 2;

    let code_exts = [".rs", ".ts", ".tsx", ".py", ".go", ".js", ".jsx"];
    let has_code_ext = code_exts.iter().any(|&ext| q_lower.contains(ext));

    // Identifier heuristics: snake_case (with underscore) or camelCase
    let has_snake_identifier = q.split_whitespace().any(|word| {
        word.contains('_') && word.chars().any(|c| c.is_alphabetic())
    });

    let total_guidance = strong_guidance_score + if has_guidance_ext { 3 } else { 0 };
    let total_logic = logic_kw_score
        + if has_code_syntax { 3 } else { 0 }
        + if has_code_ext { 2 } else { 0 }
        + if has_snake_identifier { 2 } else { 0 };

    if total_guidance > 0 && total_logic == 0 {
        SearchIntent::Guidance
    } else if total_logic > 0 && total_guidance == 0 {
        SearchIntent::Logic
    } else if total_guidance > 0 && total_logic > 0 {
        if total_guidance >= total_logic {
            SearchIntent::Guidance
        } else {
            SearchIntent::Logic
        }
    } else {
        SearchIntent::Universal
    }
}

pub fn is_guidance_path(path: &str) -> bool {
    let lower = path.replace('\\', "/").to_lowercase();
    let trimmed = lower.trim_start_matches("./");
    trimmed.ends_with(".md")
        || trimmed.ends_with(".markdown")
        || trimmed.starts_with("docs/")
        || trimmed.contains("/docs/")
        || trimmed.starts_with("skills/")
        || trimmed.contains("/skills/")
        || trimmed.starts_with(".agents/")
        || trimmed.contains("/.agents/")
}

pub fn is_test_path(path: &str) -> bool {
    let lower = path.replace('\\', "/").to_lowercase();
    let trimmed = lower.trim_start_matches("./");
    trimmed.starts_with("tests/")
        || trimmed.contains("/tests/")
        || trimmed.starts_with("test/")
        || trimmed.contains("/test/")
        || trimmed.starts_with("fixtures/")
        || trimmed.contains("/fixtures/")
        || trimmed.starts_with("mocks/")
        || trimmed.contains("/mocks/")
        || trimmed.ends_with("_test.rs")
        || trimmed.ends_with("_tests.rs")
        || trimmed.ends_with(".test.ts")
        || trimmed.ends_with(".spec.ts")
        || trimmed.ends_with("_test.go")
        || trimmed.starts_with("test_")
}

pub fn is_utility_path(path: &str) -> bool {
    let lower = path.replace('\\', "/").to_lowercase();
    let trimmed = lower.trim_start_matches("./");
    trimmed.starts_with("utils/")
        || trimmed.contains("/utils/")
        || trimmed.starts_with("util/")
        || trimmed.contains("/util/")
        || trimmed.starts_with("helpers/")
        || trimmed.contains("/helpers/")
        || trimmed.starts_with("common/")
        || trimmed.contains("/common/")
        || trimmed.ends_with("helpers.rs")
        || trimmed.ends_with("helper.rs")
        || trimmed.ends_with("utils.rs")
        || trimmed.ends_with("util.rs")
        || trimmed.ends_with("common.rs")
}
