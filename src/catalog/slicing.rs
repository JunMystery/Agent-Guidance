use crate::catalog::language_detector::ProjectLanguageProfile;
use crate::optimizer::compressor::compress_markdown;

#[derive(Debug, Clone)]
pub struct MarkdownSection {
    pub title: String,
    pub content: String,
}

/// Strips YAML frontmatter (`---\n...\n---`) from the beginning of markdown content.
pub fn strip_yaml_frontmatter(md: &str) -> &str {
    let trimmed = md.strip_prefix('\u{feff}').unwrap_or(md).trim_start();
    if let Some(after_open) = trimmed.strip_prefix("---") {
        let after_open = after_open.trim_start_matches([' ', '\t']);
        let after_first = match after_open
            .strip_prefix("\r\n")
            .or_else(|| after_open.strip_prefix('\n'))
        {
            Some(rest) => rest,
            None => return md,
        };
        for (idx, _) in after_first.match_indices("---") {
            let before = &after_first[..idx];
            let after = after_first[idx + 3..].trim_start_matches([' ', '\t']);
            let valid_before = before.ends_with('\n') || before.ends_with("\r\n") || idx == 0;
            let valid_after = after.starts_with('\n') || after.starts_with("\r\n") || after.is_empty();
            if valid_before && valid_after {
                return after.trim_start_matches(|c| c == '\r' || c == '\n');
            }
        }
    }
    md
}

/// Splits a Markdown document into logical sections based on headers (`#`, `##`, `###`).
pub fn split_markdown_into_sections(md: &str) -> Vec<MarkdownSection> {
    let clean_md = strip_yaml_frontmatter(md);
    let mut sections = Vec::new();
    let mut current_title = String::new();
    let mut current_lines: Vec<&str> = Vec::new();
    let mut open_fence: Option<(char, usize)> = None;

    for line in clean_md.lines() {
        let is_fence = crate::optimizer::compressor::update_code_fence(line, &mut open_fence);
        if is_fence || open_fence.is_some() {
            current_lines.push(line);
            continue;
        }

        let trimmed = line.trim();
        if trimmed.starts_with("# ") || trimmed.starts_with("## ") || trimmed.starts_with("### ") {
            let has_content = current_lines.iter().any(|l| !l.trim().is_empty());
            let header_text = trimmed.trim_start_matches('#').trim().to_string();

            if has_content {
                let title = if current_title.is_empty() {
                    "Overview".to_string()
                } else {
                    current_title
                };
                sections.push(MarkdownSection {
                    title,
                    content: current_lines.join("\n"),
                });
                current_lines.clear();
                current_title = header_text;
            } else {
                current_lines.clear();
                if current_title.is_empty() {
                    current_title = header_text;
                } else {
                    current_title = format!("{} - {}", current_title, header_text);
                }
            }
        } else {
            current_lines.push(line);
        }
    }

    if current_lines.iter().any(|l| !l.trim().is_empty()) {
        let title = if current_title.is_empty() {
            "Overview".to_string()
        } else {
            current_title
        };
        sections.push(MarkdownSection {
            title,
            content: current_lines.join("\n"),
        });
    }

    sections
}

/// Slices a raw Markdown skill document by task context using fast token-overlap and title relevance scoring.
/// Returns the top-k most relevant sections compressed to minimize token consumption (< 1ms).
pub fn slice_skill_markdown(raw_md: &str, task: &str, top_k: usize) -> String {
    let clean_md = strip_yaml_frontmatter(raw_md);

    // If skill is compact (<= 250 LOC AND <= 2400 tokens), preserve full instructions without destructive slicing.
    if (clean_md.lines().count() <= 250
        && crate::optimizer::compressor::estimate_tokens(clean_md, false) <= 2400)
        || task.trim().is_empty()
    {
        return compress_markdown(clean_md);
    }

    let sections: Vec<MarkdownSection> = split_markdown_into_sections(clean_md)
        .into_iter()
        .filter(|s| !s.content.trim().is_empty())
        .collect();

    let effective_top_k = top_k.max(5);

    if sections.len() <= effective_top_k {
        return compress_markdown(clean_md);
    }

    // Extract significant keywords from task (len >= 3, excluding common stop words)
    let stop_words = ["the", "and", "for", "with", "this", "that", "from", "have", "been", "will", "your", "about"];
    let keywords: Vec<String> = task
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3 && !stop_words.contains(w))
        .map(|s| s.to_string())
        .collect();

    let accumulate_sections = |secs: &[&MarkdownSection]| -> String {
        let mut acc = Vec::new();
        let mut token_acc = 0;
        for s in secs {
            let comp = compress_markdown(&s.content);
            let toks = crate::optimizer::compressor::estimate_tokens(&comp, false);
            if !acc.is_empty() && token_acc + toks > 800 {
                break;
            }
            token_acc += toks;
            acc.push(format!("#### {}\n{}", s.title, comp));
        }
        acc.join("\n\n---\n\n")
    };

    if keywords.is_empty() {
        let borrowed: Vec<&MarkdownSection> = sections.iter().take(effective_top_k).collect();
        return accumulate_sections(&borrowed);
    }

    let mut scored: Vec<(f32, &MarkdownSection)> = Vec::new();
    for sec in &sections {
        let title_lower = sec.title.to_lowercase();
        let content_lower = sec.content.to_lowercase();
        let mut score = 0.0f32;

        if title_lower.contains("overview") || title_lower.contains("quick start") || title_lower.contains("key rules") {
            score += 0.5;
        }

        for kw in &keywords {
            if title_lower.contains(kw) {
                score += 3.0;
            }
            if content_lower.contains(kw) {
                score += 1.0;
            }
        }

        if score > 0.0 {
            scored.push((score, sec));
        }
    }

    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    if scored.is_empty() {
        let borrowed: Vec<&MarkdownSection> = sections.iter().take(effective_top_k).collect();
        accumulate_sections(&borrowed)
    } else {
        let borrowed: Vec<&MarkdownSection> = scored.into_iter().take(effective_top_k).map(|(_, s)| s).collect();
        accumulate_sections(&borrowed)
    }
}

/// Returns language-specific safety micro-guidance based on detected repository profile.
pub fn get_language_safety_rules(profile: &ProjectLanguageProfile) -> String {
    let mut rules = Vec::new();

    if profile.primary_languages.contains("rust") {
        rules.push("- **Rust Safety**: Avoid `.unwrap()` / `.expect()` in production; handle `Option`/`Result` cleanly; minimize unneeded `.clone()`.");
    }
    if profile.primary_languages.contains("typescript") || profile.primary_languages.contains("javascript") {
        rules.push("- **TS/JS Safety**: Use strict types (prefer `unknown` over `any`); use optional chaining `?.` and nullish coalescing `??`.");
    }
    if profile.primary_languages.contains("python") {
        rules.push("- **Python Safety**: Use explicit type hints; handle `None` checks explicitly; avoid mutable default arguments in functions.");
    }
    if profile.primary_languages.contains("go") {
        rules.push("- **Go Safety**: Check `if err != nil` explicitly; bind goroutine lifecycles to `context.Context`; avoid data races on shared structs.");
    }
    if profile.primary_languages.contains("kotlin") || profile.primary_languages.contains("java") {
        rules.push("- **Kotlin/Java Safety**: Avoid forced unwraps (`!!`); scope coroutines properly; respect lifecycle state flows.");
    }

    if rules.is_empty() {
        "- **General Code Safety**: Validate non-null inputs before dereference; enforce explicit error handling boundaries.".to_string()
    } else {
        rules.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_safety_rules() {
        let mut profile = ProjectLanguageProfile::default();
        profile.primary_languages.insert("rust".to_string());
        let rules = get_language_safety_rules(&profile);
        assert!(rules.contains("Rust Safety"));
        assert!(rules.contains("unwrap()"));

        let mut py_profile = ProjectLanguageProfile::default();
        py_profile.primary_languages.insert("python".to_string());
        let py_rules = get_language_safety_rules(&py_profile);
        assert!(py_rules.contains("Python Safety"));
    }

    #[test]
    fn test_slice_skill_markdown_fallback() {
        let md = "# Overview\nThis is general overview\n\n## Section A\nDetails about domain usecases\n\n## Section B\nDetails about database migrations";
        let sliced = slice_skill_markdown(md, "domain usecases", 2);
        assert!(sliced.contains("Section A") || sliced.contains("Overview") || sliced.contains("usecases"));
    }

    #[test]
    fn test_slice_skill_markdown_budget_and_scoring() {
        let md = "# Overview\nLots of boilerplate overview...\n\n## Core Logic\nSpecific query target here.\n\n## Another Section\nOther text.";
        let sliced = slice_skill_markdown(md, "target", 1);
        assert!(sliced.contains("Core Logic"));
        assert!(!sliced.contains("Relevance:"));
    }

    #[test]
    fn test_strip_yaml_frontmatter() {
        let raw = "---\nname: my-skill\ndescription: A test skill\n---\n# Real Title\nBody content";
        let stripped = strip_yaml_frontmatter(raw);
        assert_eq!(stripped, "# Real Title\nBody content");

        let no_frontmatter = "# Just Title\nBody";
        assert_eq!(strip_yaml_frontmatter(no_frontmatter), no_frontmatter);
    }

    #[test]
    fn test_no_hollow_sections() {
        let md = "---\nname: test\n---\n# Parent\n\n## Child\nChild body text";
        let sections = split_markdown_into_sections(md);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].title, "Parent - Child");
        assert!(sections[0].content.contains("Child body text"));
    }

    #[test]
    fn test_compact_skill_preserves_full_content() {
        let md = "---\nname: inc\n---\n# Incremental Implementation\n\n## Overview\nBuild slices.\n\n## Rules\nRule 0: Simplicity.";
        let result = slice_skill_markdown(md, "implement feature", 2);
        assert!(result.contains("Build slices."));
        assert!(result.contains("Rule 0: Simplicity."));
        assert!(!result.contains("name: inc"));
    }

    #[test]
    fn test_strip_yaml_frontmatter_rejects_non_frontmatter_dashes_and_handles_bom() {
        let rule_line = "------\n# Heading\nBody";
        assert_eq!(strip_yaml_frontmatter(rule_line), rule_line);

        let not_yaml = "---not-frontmatter\nfoo\n---\nbar";
        assert_eq!(strip_yaml_frontmatter(not_yaml), not_yaml);

        let with_bom = "\u{feff}---\nname: bom-skill\n---\n# Title\nBody";
        assert_eq!(strip_yaml_frontmatter(with_bom), "# Title\nBody");

        let empty_fm = "---\n---\n# Body";
        assert_eq!(strip_yaml_frontmatter(empty_fm), "# Body");
    }

    #[test]
    fn test_split_markdown_ignores_headings_inside_code_fences() {
        let md = "# Deploy\n```bash\n# Build image\ndocker build .\n## Push image\ndocker push app\n```\nDone.\n\n## Next\nOther";
        let sections = split_markdown_into_sections(md);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].title, "Deploy");
        assert!(sections[0].content.contains("# Build image"));
        assert!(sections[0].content.contains("## Push image"));
        assert_eq!(sections[1].title, "Next");
    }

    #[test]
    fn test_slice_skill_markdown_slices_prose_heavy_under_250_lines() {
        let filler = "This paragraph describes unrelated deployment details and historical background. ".repeat(42);
        let mut md = String::from("# Skill\n\n## Overview\nCore summary.\n");
        for i in 1..=8 {
            md.push_str(&format!("\n## Unrelated Section {}\n{}\n", i, filler));
        }
        md.push_str("\n## Target Auth Flow\nUse JWT verification here.\n");
        assert!(md.lines().count() <= 250);
        assert!(crate::optimizer::compressor::estimate_tokens(&md, false) > 2400);

        let sliced = slice_skill_markdown(&md, "jwt auth flow", 5);
        assert!(sliced.contains("Target Auth Flow"));
        assert!(crate::optimizer::compressor::estimate_tokens(&sliced, false) <= 850);
    }
}
