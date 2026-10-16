use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::path::Path;
use tiny_http::Request;

use crate::config::current_config;
use crate::config::project_rules::{delete_project_rules, load_project_rules, save_project_rules};
use crate::config::schema::{ToolRuleItem, ToolRulesConfig};
use crate::config::ConfigWatcher;

pub const CORE_TOOLS: [&str; 6] = [
    "task_pipeline",
    "select_skills",
    "guidance",
    "project_context",
    "workflow_gate",
    "session_continuity",
];

#[derive(Deserialize)]
struct SaveRulesRequest {
    rules: HashMap<String, ToolRuleItem>,
}

fn extract_project_query(request: &Request) -> Option<String> {
    let url = request.url();
    let query_str = url.split_once('?').map(|(_, q)| q)?;
    for pair in query_str.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == "project" {
                let decoded = super::router::url_decode(v);
                if !decoded.trim().is_empty() && decoded != "all" {
                    return Some(decoded);
                }
            }
        }
    }
    None
}

pub fn handle_api_tool_rules(mut request: Request) {
    let project_arg = extract_project_query(&request);

    match request.method() {
        tiny_http::Method::Get => {
            let defaults = ToolRulesConfig::default_rules();
            let global_cfg = current_config();
            let project_rules = project_arg
                .as_deref()
                .and_then(|p| load_project_rules(Path::new(p)));

            let mut tools_list = Vec::new();
            for tool in CORE_TOOLS {
                let default_item = defaults
                    .get(tool)
                    .cloned()
                    .unwrap_or_else(|| ToolRuleItem::new(true, ""));
                let global_item = global_cfg.tool_rules.get_rule(tool).cloned();
                let project_item = project_rules
                    .as_ref()
                    .and_then(|pr| pr.get_rule(tool))
                    .cloned();

                let (effective_item, scope, is_overridden) = if let Some(p_item) = project_item.clone() {
                    (p_item, "project", true)
                } else if let Some(g_item) = global_item.clone() {
                    (g_item, "global", false)
                } else {
                    (default_item.clone(), "default", false)
                };

                tools_list.push(json!({
                    "name": tool,
                    "rule": effective_item.rule,
                    "enabled": effective_item.enabled,
                    "scope": scope,
                    "is_overridden": is_overridden,
                    "default_rule": default_item.rule,
                    "default_enabled": default_item.enabled,
                    "global_rule": global_item.as_ref().map(|i| &i.rule),
                    "global_enabled": global_item.as_ref().map(|i| i.enabled),
                    "project_rule": project_item.as_ref().map(|i| &i.rule),
                    "project_enabled": project_item.as_ref().map(|i| i.enabled),
                }));
            }

            let payload = json!({
                "status": "ok",
                "project": project_arg,
                "has_project_override": project_rules.is_some(),
                "tools": tools_list
            });
            super::router::json_response(request, 200, &payload);
        }
        tiny_http::Method::Post => {
            let mut body = String::new();
            if let Err(e) = request.as_reader().read_to_string(&mut body) {
                let err = json!({ "status": "error", "message": format!("Failed to read body: {}", e) });
                super::router::json_response(request, 400, &err);
                return;
            }

            let req_data: SaveRulesRequest = match serde_json::from_str(&body) {
                Ok(data) => data,
                Err(e) => {
                    let err = json!({ "status": "error", "message": format!("Invalid JSON: {}", e) });
                    super::router::json_response(request, 400, &err);
                    return;
                }
            };

            if let Some(ref proj_path) = project_arg {
                let cfg = ToolRulesConfig {
                    rules: req_data.rules,
                };
                if let Err(e) = save_project_rules(Path::new(proj_path), &cfg) {
                    let err = json!({ "status": "error", "message": format!("Failed to save project rules: {}", e) });
                    super::router::json_response(request, 500, &err);
                    return;
                }
                let payload = json!({
                    "status": "ok",
                    "scope": "project",
                    "project": proj_path,
                    "message": "Project tool rules saved successfully"
                });
                super::router::json_response(request, 200, &payload);
            } else {
                let mut app_cfg = (*current_config()).clone();
                app_cfg.tool_rules.rules = req_data.rules;
                if let Err(e) = ConfigWatcher::global().update(app_cfg) {
                    let err = json!({ "status": "error", "message": format!("Failed to update global config: {}", e) });
                    super::router::json_response(request, 500, &err);
                    return;
                }
                let payload = json!({
                    "status": "ok",
                    "scope": "global",
                    "message": "Global tool rules saved successfully"
                });
                super::router::json_response(request, 200, &payload);
            }
        }
        _ => {
            super::router::json_response(request, 405, &json!({ "error": "Method not allowed" }));
        }
    }
}

pub fn handle_api_tool_rules_revert(request: Request) {
    if request.method() != &tiny_http::Method::Post {
        super::router::json_response(request, 405, &json!({ "error": "POST required" }));
        return;
    }

    let project_arg = extract_project_query(&request);
    let proj_path = match project_arg {
        Some(p) => p,
        None => {
            let err = json!({ "status": "error", "message": "Missing 'project' parameter" });
            super::router::json_response(request, 400, &err);
            return;
        }
    };

    match delete_project_rules(Path::new(&proj_path)) {
        Ok(deleted) => {
            let payload = json!({
                "status": "ok",
                "deleted": deleted,
                "project": proj_path,
                "message": "Project override deleted. Reverted to global rules."
            });
            super::router::json_response(request, 200, &payload);
        }
        Err(e) => {
            let err = json!({ "status": "error", "message": format!("Failed to delete project rules: {}", e) });
            super::router::json_response(request, 500, &err);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_core_tools_length() {
        assert_eq!(CORE_TOOLS.len(), 6);
        assert!(CORE_TOOLS.contains(&"task_pipeline"));
        assert!(CORE_TOOLS.contains(&"project_context"));
    }
}
