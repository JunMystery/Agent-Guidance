use super::document::SkillSemanticDocument;

/// Extracts structured semantic fields from a skill's Markdown / YAML content.
pub fn extract_semantic_document(name: &str, content: &str) -> SkillSemanticDocument {
    let mut doc = SkillSemanticDocument {
        name: name.to_string(),
        ..Default::default()
    };

    let clean_content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let all_lines: Vec<&str> = clean_content.lines().collect();
    let mut frontmatter_lines = Vec::new();
    let mut body_lines = Vec::new();

    // Only parse YAML frontmatter if an opening `---` (after optional leading blank lines)
    // has a matching closing `---`; otherwise treat the entire document as body.
    let first_non_empty = all_lines.iter().position(|l| !l.trim().is_empty());
    let closed_frontmatter = first_non_empty.and_then(|start| {
        if all_lines[start].trim() == "---" {
            all_lines[start + 1..]
                .iter()
                .position(|l| l.trim() == "---")
                .map(|rel_end| (start, start + 1 + rel_end))
        } else {
            None
        }
    });

    if let Some((start, end)) = closed_frontmatter {
        frontmatter_lines.extend_from_slice(&all_lines[start + 1..end]);
        body_lines.extend_from_slice(&all_lines[end + 1..]);
    } else {
        body_lines.extend_from_slice(&all_lines);
    }

    // Parse Frontmatter
    let mut current_key = String::new();
    let mut desc_accumulator = Vec::new();

    for line in &frontmatter_lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let is_indented = line.starts_with(' ') || line.starts_with('\t');

        if !is_indented {
            if let Some(rest) = trimmed.strip_prefix("name:") {
                let parsed_name = rest.trim().trim_matches('"').trim_matches('\'').trim();
                if !parsed_name.is_empty() {
                    doc.name = parsed_name.to_string();
                }
                current_key = "name".to_string();
                continue;
            } else if let Some(rest) = trimmed.strip_prefix("description:") {
                current_key = "description".to_string();
                let desc_val = rest.trim().trim_matches('"').trim_matches('\'');
                if !desc_val.is_empty() && !matches!(desc_val, ">" | "|" | ">-" | "|-" | ">+" | "|+") {
                    desc_accumulator.push(desc_val.to_string());
                }
                continue;
            } else if let Some(rest) = trimmed.strip_prefix("tools:").or_else(|| trimmed.strip_prefix("tags:")) {
                current_key = "tags".to_string();
                for t in rest.split(',') {
                    let cleaned = t.trim().trim_matches('[').trim_matches(']').trim_matches('"').trim_matches('\'');
                    if !cleaned.is_empty() {
                        doc.keywords.push(cleaned.to_string());
                    }
                }
                continue;
            } else if trimmed.contains(':') && !trimmed.starts_with("- ") {
                current_key.clear();
                continue;
            }
        }

        if let Some(rest) = trimmed.strip_prefix("- ") {
            if current_key == "tags" {
                let tag = rest.trim().trim_matches('"').trim_matches('\'');
                if !tag.is_empty() {
                    doc.keywords.push(tag.to_string());
                }
            } else if current_key == "description" {
                desc_accumulator.push(rest.trim().to_string());
            }
        } else if current_key == "description" {
            desc_accumulator.push(trimmed.to_string());
        }
    }

    if !desc_accumulator.is_empty() {
        doc.description = desc_accumulator.join(" ");
    }

    // Parse Markdown Body (ignoring lines inside fenced code blocks)
    let mut in_trigger_section = false;
    let mut in_rules_section = false;
    let mut trigger_count = 0;
    let mut first_heading_captured = false;
    let mut body_text_collector = Vec::new();
    let mut open_fence: Option<(char, usize)> = None;

    for line in &body_lines {
        if crate::optimizer::compressor::update_code_fence(line, &mut open_fence) || open_fence.is_some() {
            continue;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let (false, Some(title)) = (first_heading_captured, trimmed.strip_prefix("# ")) {
            doc.title = title.trim().to_string();
            first_heading_captured = true;
            continue;
        }

        if trimmed.starts_with("## ") || trimmed.starts_with("### ") {
            let heading_title = trimmed.trim_start_matches('#').trim().to_string();
            let lower = heading_title.to_lowercase();
            doc.headings.push(heading_title);

            in_trigger_section = lower.contains("when to")
                || lower.contains("activate")
                || lower.contains("use when")
                || lower.contains("trigger")
                || lower.contains("intent")
                || lower.contains("overview");

            in_rules_section = lower.contains("guideline")
                || lower.contains("rule")
                || lower.contains("standard")
                || lower.contains("practice")
                || lower.contains("core concept")
                || lower.contains("defense")
                || lower.contains("pattern");
            continue;
        }

        if let Some(item) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .map(str::trim)
        {
            if in_trigger_section {
                if !item.is_empty() && trigger_count < 8 {
                    doc.triggers.push(item.to_string());
                    trigger_count += 1;
                }
                continue;
            } else if in_rules_section {
                if !item.is_empty() && doc.micro_rules.len() < 3 && item.len() < 140 {
                    doc.micro_rules.push(item.to_string());
                }
                continue;
            }
        }
        if body_text_collector.len() < 6 && !trimmed.starts_with('#') {
            body_text_collector.push(trimmed);
        }
    }

    if doc.description.is_empty() && !body_text_collector.is_empty() {
        doc.description = body_text_collector.join(" ");
    }

    // Set concise Intent
    doc.intent = if !doc.description.is_empty() {
        doc.description.split('.').next().unwrap_or(&doc.description).trim().to_string()
    } else if !doc.title.is_empty() {
        doc.title.clone()
    } else {
        doc.name.replace('-', " ")
    };

    // Fallback micro_rules from triggers or description
    if doc.micro_rules.is_empty() {
        for tr in doc.triggers.iter().take(3) {
            if tr.len() < 140 {
                doc.micro_rules.push(tr.clone());
            }
        }
    }
    if doc.micro_rules.is_empty() && !doc.description.is_empty() {
        doc.micro_rules.push(doc.intent.clone());
    }

    // Infer Action Triggers, File Patterns, and Applicable Phases
    doc.infer_metadata();

    // Build summary snippet
    doc.summary_snippet = doc.to_passage(1500);
    doc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_folded_block_scalar_and_colons_in_description() {
        let md = "---\nname: adaptive-language\ndescription: >-\n  Adapt tone carefully.\n  Note: preserve accuracy.\nlicense: MIT\n---\n\n# Adaptive Language\n";
        let doc = extract_semantic_document("adaptive-language", md);
        assert_eq!(doc.description, "Adapt tone carefully. Note: preserve accuracy.");
        assert_eq!(doc.intent, "Adapt tone carefully");
        assert!(!doc.description.contains("MIT"));
    }

    #[test]
    fn test_unclosed_leading_dashes_and_empty_name_and_code_fences() {
        let unclosed = "---\n# Real Heading\nSome useful guidance.\n";
        let doc_unclosed = extract_semantic_document("fallback-name", unclosed);
        assert_eq!(doc_unclosed.title, "Real Heading");
        assert!(doc_unclosed.description.contains("Some useful guidance."));

        let fenced = "\u{feff}---\nname: \n---\n```bash\n# Fake Title\n## When to Activate\n- Fake trigger\n```\n# Actual Title\n\n## Guidelines\n- Real rule\n```yaml\n- fake-code-rule\n```\n";
        let doc_fenced = extract_semantic_document("fallback-name", fenced);
        assert_eq!(doc_fenced.name, "fallback-name");
        assert_eq!(doc_fenced.title, "Actual Title");
        assert!(doc_fenced.triggers.is_empty());
        assert_eq!(doc_fenced.micro_rules, vec!["Real rule".to_string()]);
        assert_eq!(doc_fenced.headings, vec!["Guidelines".to_string()]);
    }
}
