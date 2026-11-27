use std::collections::HashSet;
use super::document::SkillSemanticDocument;
use crate::catalog::language_detector::{contains_word, contains_word_prefix};

/// Infers action triggers, file patterns, and applicable phases from name, keywords, and content.
pub fn infer_document_metadata(doc: &mut SkillSemanticDocument) {
    let name_lower = doc.name.to_lowercase();
    let desc_lower = doc.description.to_lowercase();
    let triggers_lower = doc.triggers.join(" ").to_lowercase();
    let combined = format!("{} {} {}", name_lower, desc_lower, triggers_lower);

    let mut actions = HashSet::new();
    let mut patterns = HashSet::new();
    let mut phases = HashSet::new();

    // 1. Action Triggers
    // Match mode: 0 = substring, 1 = word prefix, 2 = whole word
    let domain_actions: [(&str, &str, u8); 17] = [
        ("sql", "query", 0), ("database", "migration", 0), ("db", "database", 2),
        ("docker", "container", 0), ("k8s", "deploy", 2), ("test", "testing", 0),
        ("bench", "benchmark", 1), ("auth", "security", 1), ("security", "audit", 0),
        ("clean", "refactor", 1), ("perf", "optimize", 1), ("async", "concurrency", 1),
        ("api", "endpoint", 1), ("error", "error-handling", 0),
        ("cache", "caching", 1), ("jwt", "authentication", 2), ("orm", "data-model", 2),
    ];
    for (kw, act, mode) in domain_actions {
        let matched = match mode {
            2 => contains_word(&combined, kw),
            1 => contains_word_prefix(&combined, kw),
            _ => combined.contains(kw),
        };
        if matched {
            actions.insert(act.to_string());
            actions.insert(kw.to_string());
        }
    }
    if ["log", "logs", "logging", "logger"]
        .iter()
        .any(|w| contains_word(&combined, w))
    {
        actions.insert("telemetry".to_string());
        actions.insert("log".to_string());
    }

    for word in doc.name.replace('-', " ").replace('_', " ").split_whitespace() {
        let lower = word.to_lowercase();
        if lower.len() >= 3 && !matches!(lower.as_str(), "and" | "the" | "for" | "with" | "cheat" | "sheet") {
            actions.insert(lower);
        }
    }

    // 2. File Patterns
    if combined.contains("sql") || combined.contains("database") || combined.contains("migration") {
        patterns.insert("*.sql".to_string());
        patterns.insert("*repo*".to_string());
        patterns.insert("*migration*".to_string());
    }
    if combined.contains("docker") || combined.contains("container") {
        patterns.insert("Dockerfile*".to_string());
        patterns.insert("compose*.yml".to_string());
    }
    if combined.contains("test") {
        patterns.insert("*_test.*".to_string());
        patterns.insert("tests/*".to_string());
    }
    if contains_word_prefix(&combined, "api") || combined.contains("route") || combined.contains("endpoint") {
        patterns.insert("routes/*".to_string());
        patterns.insert("controllers/*".to_string());
    }
    if name_lower.contains("rust") { patterns.insert("*.rs".to_string()); }
    if name_lower.contains("python") || name_lower.contains("django") || name_lower.contains("fastapi") { patterns.insert("*.py".to_string()); }
    if name_lower.contains("react") || name_lower.contains("vue") || name_lower.contains("typescript") {
        patterns.insert("*.ts".to_string());
        patterns.insert("*.tsx".to_string());
    }
    if contains_word(&name_lower, "go") || name_lower.contains("golang") { patterns.insert("*.go".to_string()); }

    // 3. Applicable Phases
    if combined.contains("test") {
        phases.insert("test".to_string());
        phases.insert("review".to_string());
    }
    if combined.contains("debug") || combined.contains("error") || combined.contains("recovery") {
        phases.insert("debug".to_string());
        phases.insert("implement".to_string());
    }
    if combined.contains("plan") || combined.contains("architecture") || combined.contains("standards") {
        phases.insert("plan".to_string());
        phases.insert("review".to_string());
    }
    if phases.is_empty() {
        phases.insert("implement".to_string());
        phases.insert("refactor".to_string());
    }

    doc.action_triggers = actions.into_iter().collect();
    doc.action_triggers.sort();
    doc.file_patterns = patterns.into_iter().collect();
    doc.file_patterns.sort();
    doc.applicable_phases = phases.into_iter().collect();
    doc.applicable_phases.sort();

    let mut kw_set: HashSet<String> = doc.keywords.iter().cloned().collect();
    for act in &doc.action_triggers {
        kw_set.insert(act.clone());
    }
    doc.keywords = kw_set.into_iter().collect();
    doc.keywords.sort();
}
