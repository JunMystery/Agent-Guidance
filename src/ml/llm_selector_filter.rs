use crate::catalog::language_detector::ProjectLanguageProfile;
use crate::catalog::store::SkillItem;

/// Checks whether a candidate skill is compatible with the project profile and prompt.
pub fn is_skill_candidate_compatible(
    skill: &SkillItem,
    task_lower: &str,
    profile: &ProjectLanguageProfile,
) -> bool {
    let name_lower = skill.name.to_lowercase();
    let relative_lower = skill.relative_path.to_lowercase();

    // Direct mention in prompt bypasses filtering (unless generic stopword)
    if task_lower.contains(&name_lower) && !super::is_generic_skill_stopword(&name_lower) {
        return true;
    }

    // Negative domain filter: reject media, video, mobile, 3d, game skills unless explicitly requested
    let specialized_domains = [
        ("video", "video"),
        ("audio", "audio"),
        ("media", "media"),
        ("manim", "video"),
        ("flutter", "flutter"),
        ("android", "android"),
        ("ios", "ios"),
        ("blender", "3d"),
        ("unity", "3d"),
        ("godot", "game"),
    ];
    for (kw, domain) in specialized_domains {
        if name_lower.contains(kw) || relative_lower.contains(kw) {
            let explicitly_requested = task_lower.contains(kw) || task_lower.contains(domain);
            let project_supports = profile.secondary_tech.contains(domain)
                || profile.primary_languages.contains(domain);
            if !explicitly_requested && !project_supports {
                return false;
            }
        }
    }

    // Universal/general skills always pass (removed 'media')
    let universal_keywords = [
        "workflow", "guidance", "agent", "flow", "test", "review", "git", "cost", "doc",
    ];
    if universal_keywords
        .iter()
        .any(|k| name_lower.contains(k) || relative_lower.contains(k))
    {
        return true;
    }

    // Check language affinity
    let lang_keywords = [
        ("rust", "rust"),
        ("python", "python"),
        ("javascript", "javascript"),
        ("typescript", "typescript"),
        ("go", "go"),
        ("golang", "go"),
        ("java", "java"),
        ("cpp", "c++"),
        ("c++", "c++"),
        ("ruby", "ruby"),
        ("php", "php"),
    ];

    let mut skill_langs = Vec::new();
    for (kw, lang) in lang_keywords {
        if name_lower.contains(kw) || relative_lower.contains(kw) {
            skill_langs.push(lang);
        }
    }

    if !skill_langs.is_empty() && !profile.primary_languages.is_empty() {
        let matches_primary = skill_langs
            .iter()
            .any(|l| profile.primary_languages.contains(*l));
        if !matches_primary {
            return false;
        }
    }

    // Check domain tech affinity (e.g. database, sql, docker, frontend)
    let domain_keywords = [
        ("database", "database"),
        ("sql", "sql"),
        ("docker", "docker"),
        ("devops", "devops"),
        ("frontend", "frontend"),
        ("web", "web"),
        ("bash", "bash"),
        ("shell", "shell"),
    ];

    for (kw, domain) in domain_keywords {
        if name_lower.contains(kw) || relative_lower.contains(kw) {
            if !profile.secondary_tech.is_empty() && !profile.secondary_tech.contains(domain) {
                return false;
            }
        }
    }

    true
}
