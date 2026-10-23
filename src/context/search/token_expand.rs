//! Query token expansion and qualified identifier decomposition.

#[derive(Debug, Clone, Default)]
pub struct QueryExpansion {
    #[allow(dead_code)]
    pub raw_query: String,
    pub qualifier: Option<String>,
    pub symbol_name: String,
    pub case_variants: Vec<String>,
}

pub fn analyze_query_tokens(query: &str) -> QueryExpansion {
    let clean = query.trim();
    if clean.is_empty() {
        return QueryExpansion::default();
    }

    let (qualifier, symbol_part) = if let Some((parent, member)) = clean.split_once("::") {
        (Some(parent.trim().to_string()), member.trim().trim_end_matches("()"))
    } else if let Some((obj, member)) = clean.split_once('.') {
        let is_ext = matches!(member, "rs" | "ts" | "js" | "py" | "go" | "json" | "md" | "toml" | "kt" | "java" | "yaml" | "yml" | "css" | "html");
        if !is_ext {
            (Some(obj.trim().to_string()), member.trim().trim_end_matches("()"))
        } else {
            (None, clean)
        }
    } else {
        (None, clean.trim_end_matches("()"))
    };

    let mut case_variants = Vec::new();
    let sym_clean = symbol_part.trim();
    if !sym_clean.is_empty() {
        // CamelCase to snake_case
        let snake = camel_to_snake(sym_clean);
        if snake != sym_clean && !snake.is_empty() {
            case_variants.push(snake);
        }

        // snake_case to camelCase
        let camel = snake_to_camel(sym_clean);
        if camel != sym_clean && !camel.is_empty() {
            case_variants.push(camel);
        }
    }

    QueryExpansion {
        raw_query: clean.to_string(),
        qualifier,
        symbol_name: sym_clean.to_string(),
        case_variants,
    }
}

pub fn camel_to_snake(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 4);
    let mut prev_is_upper = false;
    for (i, c) in s.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 && !prev_is_upper {
                result.push('_');
            }
            result.push(c.to_ascii_lowercase());
            prev_is_upper = true;
        } else {
            result.push(c);
            prev_is_upper = false;
        }
    }
    result
}

pub fn snake_to_camel(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = false;
    for c in s.chars() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(c.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }
    result
}

pub const CONVERSATIONAL_STOP_WORDS: &[&str] = &[
    "how", "to", "use", "what", "is", "the", "a", "an", "in", "for",
    "of", "on", "with", "do", "does", "can", "should", "guide", "docs",
    "test", "tests", "find", "search", "get", "check", "view", "run",
];

pub fn strip_conversational_words(query: &str) -> String {
    query
        .split_whitespace()
        .filter(|w| {
            let lower = w.to_lowercase();
            let clean = lower.trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
            !CONVERSATIONAL_STOP_WORDS.contains(&clean)
        })
        .collect::<Vec<_>>()
        .join(" ")
}
