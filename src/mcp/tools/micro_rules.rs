use serde_json::Value;
use std::path::Path;

use crate::config::current_config;
use crate::config::project_rules::load_project_rules;
use crate::config::schema::{ToolRuleItem, ToolRulesConfig};

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
        if let Some(cfg) = load_project_rules(p) {
            if let Some(item) = cfg.get_rule(canonical) {
                return Some((item.clone(), RuleScope::Project));
            }
        }
    }

    let global_cfg = current_config();
    if let Some(item) = global_cfg.tool_rules.get_rule(canonical) {
        return Some((item.clone(), RuleScope::Global));
    }

    let defaults = ToolRulesConfig::default_rules();
    defaults
        .get(canonical)
        .cloned()
        .map(|item| (item, RuleScope::Default))
}

pub fn inject_tool_micro_rule(
    tool_name: &str,
    project_path: Option<&str>,
    output: &mut Value,
) {
    let canonical = normalize_tool_name(tool_name);
    let (item, scope) = match resolve_effective_rule(canonical, project_path) {
        Some(res) => res,
        None => return,
    };

    if !item.enabled || item.rule.trim().is_empty() {
        return;
    }

    let banner = format!(
        "\n\n> [!IMPORTANT] **Tool Execution Mandate (`{}`) [{}]**:\n> {}",
        canonical,
        scope.label(),
        item.rule.trim().replace('\n', "\n> ")
    );

    if let Some(arr) = output.get_mut("content").and_then(|c| c.as_array_mut()) {
        for obj in arr.iter_mut() {
            if let Some(first_obj) = obj.as_object_mut() {
                if let Some(text_val) = first_obj.get_mut("text") {
                    if let Some(existing_text) = text_val.as_str() {
                        let mut updated = existing_text.to_string();
                        updated.push_str(&banner);
                        *text_val = Value::String(updated);
                        break;
                    }
                }
            }
        }
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
        assert!(text.contains("Tool Execution Mandate (`task_pipeline`) [Default Rule]"));
        assert!(text.contains("Initial output."));
    }
}
