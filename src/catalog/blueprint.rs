use std::path::Path;
#[cfg(test)]
use crate::catalog::store::SkillItem;
use crate::context::db::CodeGraphDb;

/// Generates an upfront architectural split blueprint based on the active repository code graph.
/// Analyzes existing file sizes to detect candidates for splitting (>= 200 LOC) and proposes
/// target sub-module structures aligned with the detected architectural pattern.
pub fn generate_dynamic_blueprint(proj_path: &Path, task: &str, arch_pattern: &str) -> String {
    let mut large_files = Vec::new();

    let db_path = proj_path.join(".agent-context").join("code_graph.db");
    if db_path.exists() {
        if let Ok(db) = CodeGraphDb::open_read_only(&db_path) {
            // Extract keywords from task for relevance lookup
            let words: Vec<&str> = task
                .split_whitespace()
                .filter(|w| w.len() > 3)
                .collect();

            for word in words {
                if let Ok(syms) = db.search_symbols(word, 5) {
                    for (file_path, sym_name, _line) in syms {
                        let p_lower = file_path.to_lowercase();
                        if p_lower.starts_with("skills/")
                            || p_lower.starts_with("skills\\")
                            || p_lower.starts_with(".agents")
                            || p_lower.starts_with(".claude")
                            || p_lower.starts_with(".opencode")
                            || p_lower.ends_with("_test.rs")
                            || p_lower.ends_with("_tests.rs")
                            || p_lower.ends_with(".test.ts")
                            || p_lower.ends_with(".spec.ts")
                            || p_lower.ends_with(".test.js")
                            || crate::mcp::tools::gate_edit::is_exempt_from_loc_limit(&file_path)
                        {
                            continue;
                        }

                        // Fast metadata check: if file size < 5KB, it cannot exceed 200 LOC
                        if let Ok(Some((_hash, size, _mod))) = db.get_file_metadata(&file_path) {
                            if size < 5000 {
                                continue;
                            }
                        }

                        let rel = Path::new(&file_path);
                        if rel.is_absolute()
                            || rel.components().any(|c| matches!(c, std::path::Component::ParentDir))
                        {
                            continue;
                        }
                        let full = proj_path.join(rel);
                        if let Ok(content) = std::fs::read_to_string(&full) {
                            let loc = content.lines().count();
                            if loc > 300 && !large_files.iter().any(|(p, _, _)| p == &file_path) {
                                large_files.push((file_path, sym_name, loc));
                            }
                        }
                    }
                }
            }
        }
    }

    let mut blueprint = String::new();

    if !large_files.is_empty() {
        blueprint.push_str("### Cohesion Review Candidates (> 300 LOC soft limit):\n");
        for (file, sym, loc) in &large_files {
            blueprint.push_str(&format!(
                "- `{}` ({} LOC, `{}`) — check cohesion / extract fns >60L.\n",
                file, loc, sym
            ));
        }
    }

    let layout = match arch_pattern {
        "Clean_Architecture" => "`domain/` | `usecase/` | `infrastructure/` | `entrypoint/`",
        "Layered_Architecture" => "`controllers/` | `services/` | `models/` | `repository/`",
        "Package_By_Feature" => "`<feature>/handlers.*` | `service.*` | `types.*`",
        "CLI_Pipeline" => "`main.rs` | `commands/` | `core/`",
        "Flat_Library" => "`lib.rs` | `internal/`",
        _ => "thin entrypoint | domain-grouped feature modules",
    };
    blueprint.push_str(&format!("Blueprint [{}]: {}\n", arch_pattern, layout));

    blueprint
}

/// Formats a soft modularity & domain cohesion advisory when reviewing a file > 300 LOC.
#[cfg(test)]
pub fn format_decomposition_guidance(rel_path: &str, loc: usize, arch_pattern: &str) -> String {
    let mut guidance = format!(
        "# Soft Modularity & Domain Cohesion Advisory\n\n\
        - Target File: `{}`\n\
        - Current Length: **{} lines** (soft limit: 300 lines)\n\
        - Architecture Pattern: **{}**\n\n\
        **Review Question**: *Is `{}` handling unrelated responsibilities, or does it contain overly long/complex functions (> 60 LOC, nesting > 4)?* If it is cohesive within one domain/use case and composed of small focused functions, keeping it intact is reasonable.\n\n\
        ### Domain Grouping Guide for [{}] (if splitting is warranted)\n",
        rel_path, loc, arch_pattern, rel_path, arch_pattern
    );

    match arch_pattern {
        "Clean_Architecture" => {
            guidance.push_str(
                "- Colocate domain models & interfaces in `domain/`\n\
                - Place specific business use cases in `usecase/` with small functions (15-40 LOC)\n\
                - Keep external I/O & database adapters in `infrastructure/`\n\
                - Keep entry handlers thin and focused on dispatch\n"
            );
        }
        "Layered_Architecture" => {
            guidance.push_str(
                "- Keep routing and request parsing in `controllers/`\n\
                - Keep cohesive domain workflows in `services/`\n\
                - Keep schemas and data mapping in `models/`\n"
            );
        }
        "Package_By_Feature" => {
            guidance.push_str(
                "- Group feature files together to localize edits:\n\
                - `<feature>/handlers.*`: Flow control & entry points\n\
                - `<feature>/service.*`: Core feature computations\n\
                - `<feature>/types.*`: Feature entities & error types\n"
            );
        }
        "CLI_Pipeline" => {
            guidance.push_str(
                "- Keep `main.*` focused on CLI parsing and dispatch\n\
                - Group sub-command handlers under `commands/<command_name>.*`\n\
                - Keep reusable pipelines in `core/`\n"
            );
        }
        "Orchestrator" => {
            guidance.push_str(
                "- Keep the orchestrator focused on high-level stage transitions\n\
                - Group pipeline stages into `steps/` or `workers/` by domain\n\
                - Keep pipeline state models in `types.*`\n"
            );
        }
        "Flat_Library" => {
            guidance.push_str(
                "- Keep `lib.*` as a clean public API facade\n\
                - Group internal modules under `internal/` by domain responsibility\n"
            );
        }
        _ => {
            guidance.push_str(
                "- Extract long/complex functions (> 60 LOC) into focused helpers\n\
                - Group cohesive files by domain/feature to avoid cross-module side effects\n"
            );
        }
    }

    guidance.push_str(
        "\n**Optional Refactoring**: Only split if the file mixes unrelated domains or has complex functions; otherwise keep cohesive domain logic together.\n"
    );

    guidance
}

/// Synthesizes top recommended skills into an actionable, unified step-by-step checklist.
#[cfg(test)]
pub fn generate_skill_recipe(skills: &[(f32, SkillItem)], task: &str) -> String {
    if skills.is_empty() {
        return "1. Review project standards and verify architectural boundaries.\n2. Implement targeted modifications respecting the 300 LOC limit.\n3. Run automated tests to verify behavior.".to_string();
    }

    let top_skills: Vec<&SkillItem> = skills.iter().take(3).map(|(_, item)| item).collect();
    let mut checklist = Vec::new();

    checklist.push(format!(
        "1. **Pre-Implementation**: Align task '{}' with patterns from `{}`.",
        task, top_skills[0].name
    ));

    if top_skills.len() > 1 {
        checklist.push(format!(
            "2. **Core Logic**: Apply safety and domain rules from `{}`.",
            top_skills[1].name
        ));
    }

    if top_skills.len() > 2 {
        checklist.push(format!(
            "3. **Quality & Boundary**: Enforce testing and separation guidelines from `{}`.",
            top_skills[2].name
        ));
    }

    checklist.push("4. **Verification**: Confirm changes pass empirical tests before advancing to Proposal stage.".to_string());

    checklist.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::store::SkillSource;

    #[test]
    fn test_generate_dynamic_blueprint_patterns() {
        let path = Path::new(".");
        let bp_clean = generate_dynamic_blueprint(path, "manage inventory", "Clean_Architecture");
        assert!(bp_clean.contains("Clean_Architecture"));
        assert!(bp_clean.contains("domain/"));
        assert!(bp_clean.contains("usecase/"));

        let bp_cli = generate_dynamic_blueprint(path, "cli parser", "CLI_Pipeline");
        assert!(bp_cli.contains("CLI_Pipeline"));
        assert!(bp_cli.contains("main.rs"));
        assert!(bp_cli.contains("commands/"));
    }

    #[test]
    fn test_generate_skill_recipe_steps() {
        let skills = vec![
            (0.9, SkillItem {
                name: "rust-patterns".to_string(),
                relative_path: "skills/rust.md".to_string(),
                source: SkillSource::Embedded,
                content: String::new(),
            }),
            (0.8, SkillItem {
                name: "security-audit".to_string(),
                relative_path: "skills/security.md".to_string(),
                source: SkillSource::Embedded,
                content: String::new(),
            }),
        ];

        let recipe = generate_skill_recipe(&skills, "refactor auth");
        assert!(recipe.contains("Pre-Implementation"));
        assert!(recipe.contains("rust-patterns"));
        assert!(recipe.contains("Core Logic"));
        assert!(recipe.contains("security-audit"));
        assert!(recipe.contains("Verification"));
    }
}
