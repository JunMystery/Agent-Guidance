use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};
use std::time::SystemTime;

use crate::config::current_config;
use crate::config::project_rules::{get_project_rules_path, load_project_rules};
use crate::config::schema::{ToolRuleItem, ToolRulesConfig};
use crate::mcp::state::ServerState;

type CachedRuleEntry = (Option<(SystemTime, u64)>, Option<ToolRulesConfig>);
static DEFAULT_RULES_CACHE: OnceLock<HashMap<String, ToolRuleItem>> = OnceLock::new();
static PROJECT_RULES_CACHE: OnceLock<RwLock<HashMap<PathBuf, CachedRuleEntry>>> = OnceLock::new();

fn default_rules_cached() -> &'static HashMap<String, ToolRuleItem> {
    DEFAULT_RULES_CACHE.get_or_init(ToolRulesConfig::default_rules)
}

fn load_project_rules_cached(project_path: &Path) -> Option<ToolRulesConfig> {
    let rules_path = get_project_rules_path(project_path);
    let stamp = std::fs::metadata(&rules_path)
        .ok()
        .filter(|m| m.is_file())
        .map(|m| (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len()));

    let cache = PROJECT_RULES_CACHE.get_or_init(|| RwLock::new(HashMap::new()));
    if let Ok(guard) = cache.read() {
        if let Some((cached_stamp, cached_cfg)) = guard.get(&rules_path) {
            if *cached_stamp == stamp {
                return cached_cfg.clone();
            }
        }
    }

    let loaded = if stamp.is_some() {
        load_project_rules(project_path)
    } else {
        None
    };

    if let Ok(mut guard) = cache.write() {
        if guard.len() > 64 {
            guard.clear();
        }
        guard.insert(rules_path, (stamp, loaded.clone()));
    }
    loaded
}

fn should_suppress_micro_rule(existing_text: &str) -> bool {
    let trimmed = existing_text.trim_start();
    trimmed.starts_with("# Task Pipeline Activated")
        || trimmed.starts_with("# Informational Query Detected")
        || trimmed.starts_with("# Generative UI Active")
        || trimmed.starts_with("# Slash Command Detected:")
        || trimmed.starts_with("# Workflow Stage Status")
        || trimmed.starts_with("# Workflow Gate: [check]")
        || trimmed.starts_with("# Workflow Gate: [status]")
        || trimmed.starts_with("# Skill Selection\n\nNo skills selected.")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleScope {
    Project,
    Global,
    Default,
}

impl RuleScope {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Project => "Project Rule",
            Self::Global => "Global Rule",
            Self::Default => "Default Rule",
        }
    }
}

pub fn normalize_tool_name(name: &str) -> &str {
    match name {
        "select_skill" => "select_skills",
        other => other,
    }
}

pub fn resolve_effective_rule(
    tool_name: &str,
    project_path: Option<&str>,
) -> Option<(ToolRuleItem, RuleScope)> {
    let canonical = normalize_tool_name(tool_name);
    if let Some(path_str) = project_path {
        let p = Path::new(path_str);
        if let Some(cfg) = load_project_rules_cached(p) {
            if let Some(item) = cfg.get_rule(canonical) {
                return Some((ToolRulesConfig::upgrade_legacy_rule(canonical, item), RuleScope::Project));
            }
        }
    }

    let global_cfg = current_config();
    if let Some(item) = global_cfg.tool_rules.get_rule(canonical) {
        return Some((ToolRulesConfig::upgrade_legacy_rule(canonical, item), RuleScope::Global));
    }

    default_rules_cached()
        .get(canonical)
        .cloned()
        .map(|item| (item, RuleScope::Default))
}

fn append_rule_banner(
    canonical: &str,
    scope: RuleScope,
    rule_text: &str,
    output: &mut Value,
) -> bool {
    let banner = format!(
        "\n\n> [Rule:{} ({})] {} *(User RULES take precedence)*",
        canonical,
        scope.label(),
        rule_text.trim().replace('\n', " ")
    );

    if let Some(arr) = output.get_mut("content").and_then(|c| c.as_array_mut()) {
        for obj in arr.iter_mut() {
            if let Some(first_obj) = obj.as_object_mut()
                && let Some(text_val) = first_obj.get_mut("text")
                && let Some(existing_text) = text_val.as_str()
            {
                if should_suppress_micro_rule(existing_text) {
                    return false;
                }
                let mut updated = existing_text.to_string();
                updated.push_str(&banner);
                *text_val = Value::String(updated);
                return true;
            }
        }
    }
    false
}

pub fn inject_tool_micro_rule(
    tool_name: &str,
    project_path: Option<&str>,
    output: &mut Value,
) {
    let canonical = normalize_tool_name(tool_name);
    let Some((item, scope)) = resolve_effective_rule(canonical, project_path) else {
        return;
    };
    if !item.enabled || item.rule.trim().is_empty() {
        return;
    }
    append_rule_banner(canonical, scope, &item.rule, output);
}

pub fn inject_tool_micro_rule_once(
    tool_name: &str,
    project_path: Option<&str>,
    state: &mut ServerState,
    output: &mut Value,
) {
    let canonical = normalize_tool_name(tool_name);
    let Some((item, scope)) = resolve_effective_rule(canonical, project_path) else {
        return;
    };
    if !item.enabled || item.rule.trim().is_empty() {
        return;
    }
    let normalized_rule = item.rule.trim().replace('\n', " ");
    if state.emitted_micro_rules.get(canonical).map(String::as_str) == Some(normalized_rule.as_str()) {
        return;
    }
    if append_rule_banner(canonical, scope, &item.rule, output) {
        state
            .emitted_micro_rules
            .insert(canonical.to_string(), normalized_rule);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_resolve_default_rule() {
        let res = resolve_effective_rule("task_pipeline", None);
        assert!(res.is_some());
        let (item, scope) = res.unwrap();
        assert_eq!(scope, RuleScope::Default);
        assert!(item.enabled);
        assert!(item.rule.contains("CALL FIRST"));
    }

    #[test]
    fn test_resolve_alias_tool_name() {
        let res = resolve_effective_rule("select_skill", None);
        assert!(res.is_some());
        let (item, scope) = res.unwrap();
        assert_eq!(scope, RuleScope::Default);
        assert!(item.rule.contains("Load only confirmed"));
    }

    #[test]
    fn test_inject_tool_micro_rule() {
        let mut val = json!({
            "content": [
                { "type": "resource", "uri": "test" },
                { "type": "text", "text": "Initial output." }
            ]
        });

        inject_tool_micro_rule("task_pipeline", None, &mut val);
        let text = val["content"][1]["text"].as_str().unwrap();
        assert!(text.contains("[Rule:task_pipeline (Default Rule)]"));
        assert!(text.contains("User RULES take precedence"));
        assert!(text.contains("Initial output."));
    }

    #[test]
    fn test_legacy_default_rule_auto_upgrades_while_custom_preserved() {
        let legacy = ToolRuleItem::new(
            true,
            "Read bounded code (max 300 LOC). Files >= 300 LOC are monolithic: decompose into sub-modules (< 150 LOC) rather than adding code.",
        );
        let upgraded = ToolRulesConfig::upgrade_legacy_rule("project_context", &legacy);
        assert!(upgraded.rule.contains("soft cohesion check"));

        let custom = ToolRuleItem::new(true, "My custom project rule");
        let preserved = ToolRulesConfig::upgrade_legacy_rule("project_context", &custom);
        assert_eq!(preserved.rule, "My custom project rule");
    }

    #[test]
    fn test_micro_rule_suppression_and_mtime_cache() {
        let mut qa_val = json!({
            "content": [{ "type": "text", "text": "# Informational Query Detected for 'x'\n\nAnswer directly." }]
        });
        inject_tool_micro_rule("guidance", None, &mut qa_val);
        assert!(!qa_val["content"][0]["text"].as_str().unwrap().contains("[Rule:"));

        let mut genui_val = json!({
            "content": [{ "type": "text", "text": "# Generative UI Active: cards\n\nRender HTML." }]
        });
        inject_tool_micro_rule("guidance", None, &mut genui_val);
        assert!(!genui_val["content"][0]["text"].as_str().unwrap().contains("[Rule:"));

        let mut status_val = json!({
            "content": [{ "type": "text", "text": "# Workflow Stage Status\n\n- Active Stage: Plan" }]
        });
        inject_tool_micro_rule("workflow_gate", None, &mut status_val);
        assert!(!status_val["content"][0]["text"].as_str().unwrap().contains("[Rule:"));

        let tmp = std::env::temp_dir().join(format!("ag_microrule_cache_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let ctx_dir = tmp.join(".agent-context");
        std::fs::create_dir_all(&ctx_dir).unwrap();
        let rules_file = ctx_dir.join("tool_rules.toml");
        std::fs::write(&rules_file, "[rules.guidance]\nenabled = true\nrule = \"Rule v1\"\n").unwrap();

        let (item1, scope1) = resolve_effective_rule("guidance", tmp.to_str()).unwrap();
        assert_eq!(scope1, RuleScope::Project);
        assert_eq!(item1.rule, "Rule v1");

        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&rules_file, "[rules.guidance]\nenabled = true\nrule = \"Rule v2 updated\"\n").unwrap();
        let (item2, _) = resolve_effective_rule("guidance", tmp.to_str()).unwrap();
        assert_eq!(item2.rule, "Rule v2 updated");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_micro_rule_once_dedups_per_session_and_reemits_on_rule_change() {
        let tmp = std::env::temp_dir().join(format!("ag_microrule_once_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let ctx_dir = tmp.join(".agent-context");
        std::fs::create_dir_all(&ctx_dir).unwrap();
        let rules_file = ctx_dir.join("tool_rules.toml");
        std::fs::write(&rules_file, "[rules.project_context]\nenabled = true\nrule = \"Rule v1\"\n").unwrap();

        let mut state = ServerState::new();
        let mut out1 = json!({ "content": [{ "type": "text", "text": "Read 1" }] });
        inject_tool_micro_rule_once("project_context", tmp.to_str(), &mut state, &mut out1);
        assert!(out1["content"][0]["text"].as_str().unwrap().contains("[Rule:project_context"));

        let mut out2 = json!({ "content": [{ "type": "text", "text": "Read 2" }] });
        inject_tool_micro_rule_once("project_context", tmp.to_str(), &mut state, &mut out2);
        assert_eq!(out2["content"][0]["text"].as_str().unwrap(), "Read 2");

        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&rules_file, "[rules.project_context]\nenabled = true\nrule = \"Rule v2\"\n").unwrap();
        let mut out3 = json!({ "content": [{ "type": "text", "text": "Read 3" }] });
        inject_tool_micro_rule_once("project_context", tmp.to_str(), &mut state, &mut out3);
        assert!(out3["content"][0]["text"].as_str().unwrap().contains("Rule v2"));

        let mut pipeline_out = json!({ "content": [{ "type": "text", "text": "# Task Pipeline Activated\n\nOK" }] });
        inject_tool_micro_rule_once("task_pipeline", tmp.to_str(), &mut state, &mut pipeline_out);
        assert!(!pipeline_out["content"][0]["text"].as_str().unwrap().contains("[Rule:"));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
