use anyhow::Result;
use std::fs;
use std::path::Path;
use tracing::info;

use crate::mcp::templates::{
    AGENT_GUIDANCE_SKILL_TAG_END, AGENT_GUIDANCE_SKILL_TAG_START,
    AGENT_GUIDANCE_TAG_END, AGENT_GUIDANCE_TAG_START,
};

pub(crate) fn remove_global_rules(home: &Path) -> Result<()> {
    let targets = vec![
        home.join(".gemini").join("GEMINI.md"),
        home.join(".gemini").join("config").join("AGENTS.md"),
        home.join(".config").join("opencode").join("AGENTS.md"),
        home.join(".claude").join("CLAUDE.md"),
        home.join(".codex").join("AGENTS.md"),
        home.join(".codeium").join("windsurf").join("AGENTS.md"),
    ];
    for path in &targets {
        if path.exists() {
            if let Ok(content) = fs::read_to_string(path) {
                let cleaned = strip_tagged_section(
                    &content,
                    AGENT_GUIDANCE_TAG_START,
                    AGENT_GUIDANCE_TAG_END,
                );
                if cleaned != content {
                    fs::write(path, cleaned)?;
                    info!("Cleaned rules from: {}", path.display());
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn remove_skills_enforcer(home: &Path) -> Result<()> {
    let targets = vec![
        home.join(".claude")
            .join("skills")
            .join("agent-guidance")
            .join("SKILL.md"),
        home.join(".config")
            .join("opencode")
            .join("skills")
            .join("agent-guidance")
            .join("SKILL.md"),
        home.join(".agents")
            .join("skills")
            .join("agent-guidance")
            .join("SKILL.md"),
        home.join(".codex")
            .join("skills")
            .join("agent-guidance")
            .join("SKILL.md"),
        home.join(".codeium")
            .join("windsurf")
            .join("skills")
            .join("agent-guidance")
            .join("SKILL.md"),
    ];
    for path in &targets {
        if path.exists() {
            if let Ok(content) = fs::read_to_string(path) {
                let cleaned = strip_tagged_section(
                    &content,
                    AGENT_GUIDANCE_SKILL_TAG_START,
                    AGENT_GUIDANCE_SKILL_TAG_END,
                );
                if cleaned.trim().is_empty() {
                    let _ = fs::remove_file(path);
                    info!("Removed skill enforcer: {}", path.display());
                } else if cleaned != content {
                    fs::write(path, cleaned)?;
                    info!("Cleaned skill enforcer: {}", path.display());
                }
            }
        }
    }
    Ok(())
}

fn strip_tagged_section(content: &str, start_tag: &str, end_tag: &str) -> String {
    let mut current = content.to_string();
    while let Some(start_idx) = current.find(start_tag) {
        let search_from = start_idx + start_tag.len();
        let Some(rel_end_idx) = current[search_from..].find(end_tag) else {
            break;
        };
        let end_idx = search_from + rel_end_idx;
        let before = current[..start_idx].trim_end();
        let after = current[end_idx + end_tag.len()..].trim_start();
        let mut next = String::new();
        next.push_str(before);
        if !before.is_empty() && !after.is_empty() {
            next.push('\n');
        }
        next.push_str(after);
        current = next;
    }
    current
}

#[cfg(test)]
mod tests {
    use super::strip_tagged_section;

    #[test]
    fn test_strip_tagged_section_ignores_reversed_end_tag() {
        let content = "before\n<!-- END -->\nmiddle\n<!-- START -->\nkeep";
        assert_eq!(
            strip_tagged_section(content, "<!-- START -->", "<!-- END -->"),
            content
        );
    }

    #[test]
    fn test_strip_tagged_section_removes_matched_block() {
        let content = "header\n<!-- START -->\nremove me\n<!-- END -->\nfooter";
        assert_eq!(
            strip_tagged_section(content, "<!-- START -->", "<!-- END -->"),
            "header\nfooter"
        );
    }

    #[test]
    fn test_strip_tagged_section_removes_all_blocks() {
        let content = "a\n<!-- START -->\nfirst\n<!-- END -->\nb\n<!-- START -->\nsecond\n<!-- END -->\nc";
        assert_eq!(
            strip_tagged_section(content, "<!-- START -->", "<!-- END -->"),
            "a\nb\nc"
        );
    }
}
