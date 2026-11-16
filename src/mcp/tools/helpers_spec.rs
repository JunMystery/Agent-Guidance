//! Path spec parser for Clustered Read and Semantic Slicing.
//! Parses paths with optional symbol (`#symbol`) and line range (`:start-end`).

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetPathSpec {
    pub clean_path: String,
    pub target_symbol: Option<String>,
    pub line_range: Option<(usize, usize)>,
}

impl TargetPathSpec {
    pub fn parse(raw: &str) -> Self {
        let trimmed = raw.trim().trim_matches('"').trim_matches('\'');
        if trimmed.is_empty() {
            return Self {
                clean_path: String::new(),
                target_symbol: None,
                line_range: None,
            };
        }

        // 1. Separate symbol fragment after '#'
        let (path_part, symbol_opt) = match trimmed.find('#') {
            Some(idx) => {
                let p = &trimmed[..idx];
                let s = trimmed[idx + 1..].trim();
                let sym = if s.is_empty() { None } else { Some(s.to_string()) };
                (p, sym)
            }
            None => (trimmed, None),
        };

        // 2. Separate line range after ':' (avoiding Windows drive letters like C:\)
        let (clean_p, range_opt) = parse_line_range(path_part);

        Self {
            clean_path: clean_p.trim().replace('\\', "/"),
            target_symbol: symbol_opt,
            line_range: range_opt,
        }
    }
}

fn parse_line_range(path_part: &str) -> (&str, Option<(usize, usize)>) {
    // If string has a Windows drive prefix (e.g. "C:\..."), search after the prefix
    let search_start = if path_part.len() >= 2
        && path_part.as_bytes()[1] == b':'
        && path_part.as_bytes()[0].is_ascii_alphabetic()
    {
        2
    } else {
        0
    };

    if let Some(colon_rel) = path_part[search_start..].rfind(':') {
        let colon_idx = search_start + colon_rel;
        let suffix = &path_part[colon_idx + 1..].trim();
        if let Some(range) = parse_numbers(suffix) {
            return (&path_part[..colon_idx], Some(range));
        }
    }
    (path_part, None)
}

fn parse_numbers(s: &str) -> Option<(usize, usize)> {
    if s.is_empty() {
        return None;
    }
    if let Some(dash_idx) = s.find('-') {
        let start_str = s[..dash_idx].trim();
        let end_str = s[dash_idx + 1..].trim();
        let start = start_str.parse::<usize>().ok()?;
        let end = end_str.parse::<usize>().ok()?;
        if start > 0 && end >= start {
            return Some((start, end));
        }
    } else if let Ok(single) = s.parse::<usize>() {
        if single > 0 {
            return Some((single, single));
        }
    }
    None
}
