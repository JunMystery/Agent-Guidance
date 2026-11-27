use std::path::Path;
use crate::catalog::store::get_embedded_skill;
use super::validate_path;
use super::skills_gate::clean_skill_identifier;

fn is_skill_markdown(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| matches!(e.to_ascii_lowercase().as_str(), "md" | "markdown" | "mdc"))
            .unwrap_or(false)
}

fn read_local_skill_file(proj_path: &Path, rel: &str) -> Option<(Option<String>, String)> {
    let trimmed = rel.trim();
    if trimmed.is_empty() {
        return None;
    }
    let full_path = validate_path(proj_path, trimmed).ok()?;
    if !is_skill_markdown(&full_path) {
        return None;
    }
    let content = std::fs::read_to_string(&full_path).ok()?;
    let fm_name = crate::catalog::store::scanner::extract_frontmatter_name(&content).or_else(|| {
        full_path
            .file_name()
            .and_then(|f| f.to_str())
            .filter(|f| f.eq_ignore_ascii_case("SKILL.md"))
            .and_then(|_| full_path.parent())
            .and_then(|p| p.file_name())
            .map(|f| f.to_string_lossy().to_string())
    });
    Some((fm_name, content))
}

fn find_local_override(proj_path: &Path, skill_name: &str, rel_path: Option<&str>) -> Option<(Option<String>, String)> {
    let override_candidates = [
        format!(".agents/skills/{}/SKILL.md", skill_name),
        format!(".opencode/skills/{}/SKILL.md", skill_name),
        format!(".claude/skills/{}/SKILL.md", skill_name),
    ];
    for candidate in &override_candidates {
        if let Some(found) = read_local_skill_file(proj_path, candidate) {
            return Some(found);
        }
    }
    if let Some(rel) = rel_path {
        if let Some(found) = read_local_skill_file(proj_path, rel) {
            return Some(found);
        }
    }
    None
}

fn matches_skill_identifier(skill_name: &str, rel_path: &str, clean_name: &str) -> bool {
    let clean_norm = clean_name.replace('\\', "/").trim_matches('/').to_ascii_lowercase();
    if clean_norm.is_empty() {
        return false;
    }
    if skill_name.eq_ignore_ascii_case(&clean_norm) {
        return true;
    }
    let rel_norm = rel_path.replace('\\', "/").trim_matches('/').to_ascii_lowercase();
    if rel_norm.is_empty() {
        return false;
    }
    if rel_norm == clean_norm {
        return true;
    }
    rel_norm.ends_with(&format!("/{}", clean_norm))
        || clean_norm.ends_with(&format!("/{}", rel_norm))
}

pub(crate) fn resolve_skill(
    raw_req: &str,
    proposals: &[(String, String, f32)],
    all_skills: &[crate::catalog::store::SkillItem],
    proj_path: &Path,
) -> Option<(String, String, &'static str)> {
    let clean_name = clean_skill_identifier(raw_req);
    if clean_name.is_empty() {
        return None;
    }

    // 1. Check proposals (preferring local workspace overrides when rel_path/override resolves locally)
    if let Some((prop_name, rel_path, _)) = proposals
        .iter()
        .find(|(n, rel, _)| matches_skill_identifier(n, rel, &clean_name))
    {
        if let Some((_, c)) = find_local_override(proj_path, prop_name, Some(rel_path)) {
            return Some((prop_name.clone(), c, " [Local Workspace]"));
        }
        if let Some(c) = get_embedded_skill(prop_name).or_else(|| get_embedded_skill(rel_path)) {
            return Some((prop_name.clone(), c, " [Embedded Catalog]"));
        }
    }

    // 2. Check explicit local workspace overrides (.agents/.opencode/.claude) before embedded catalog
    if let Some((fm_name, c)) = find_local_override(proj_path, &clean_name, None) {
        let canonical = fm_name.unwrap_or_else(|| clean_name.clone());
        return Some((canonical, c, " [Local Workspace]"));
    }

    // 3. Check all catalog & workspace skills using exact name or full path-segment match
    if let Some(item) = all_skills
        .iter()
        .find(|s| matches_skill_identifier(&s.name, &s.relative_path, &clean_name))
    {
        let tag = match item.source {
            crate::catalog::store::SkillSource::Embedded => " [Embedded Catalog]",
            crate::catalog::store::SkillSource::LocalWorkspace(_) => " [Local Workspace]",
        };
        return Some((item.name.clone(), item.content.clone(), tag));
    }

    // 4. Fallback to embedded skill direct lookup
    if let Some(c) = get_embedded_skill(&clean_name) {
        return Some((clean_name, c, " [Embedded Catalog]"));
    }

    // 5. Fallback to direct file path lookup (strictly markdown/rule assets)
    if let Some((fm_name, c)) = read_local_skill_file(proj_path, &clean_name) {
        let canonical = fm_name.unwrap_or_else(|| clean_name.clone());
        return Some((canonical, c, " [Local Workspace]"));
    }

    None
}
