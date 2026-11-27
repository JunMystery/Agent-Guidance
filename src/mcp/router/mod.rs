pub mod resources;
pub mod tools;

use crate::mcp::state::ServerState;
use crate::mcp::tools::handle_tool_call;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::atomic::Ordering;

const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &["2024-11-05", "2025-03-26", "2025-06-18"];

fn is_known_tool(name: &str) -> bool {
    matches!(
        name,
        "task_pipeline"
            | "select_skills"
            | "select_skill"
            | "guidance"
            | "project_context"
            | "session_continuity"
            | "workflow_gate"
    )
}

fn is_non_trivial_state(state: &ServerState) -> bool {
    state.tool_calls > 0
        || !state.modified_files.is_empty()
        || !state.recent_tasks.is_empty()
        || state.plan_approved
        || state.edit_authorized
        || state.workflow_stage != "Context"
        || state.active_task.is_some()
}

fn switch_session_state(state: &mut ServerState, clean_id: &str, proj_hint: Option<&str>) {
    if is_non_trivial_state(state)
        && ServerState::is_valid_session_id(&state.session_id)
        && let Some(ref proj_str) = state.project_path
    {
        let proj = Path::new(proj_str);
        if proj.is_dir() {
            let _ = state.save_to_dir(proj);
        }
    }

    state.reset_for_context_switch();
    state.priority_gate_passed = false;
    state.last_session_start = None;
    state.session_id = clean_id.to_string();

    let load_root = proj_hint
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != ".")
        .or(state.project_path.as_deref())
        .or_else(|| state.workspace_roots.first().map(|s| s.as_str()));

    if let Some(root_str) = load_root {
        let root_path = Path::new(root_str);
        if root_path.is_dir()
            && let Ok(mut loaded) = ServerState::load_session_by_id(root_path, clean_id)
        {
            if loaded.workspace_roots.is_empty() {
                loaded.workspace_roots = state.workspace_roots.clone();
            }
            if loaded.agent_client_name.is_none() {
                loaded.agent_client_name = state.agent_client_name.clone();
            }
            loaded.cancellation = state.cancellation.clone();
            *state = loaded;
        }
    }
}

pub fn handle_request(
    method: &str,
    params: Option<Value>,
    state: &mut ServerState,
) -> Result<Value, (i32, String)> {
    match method {
        "initialize" => {
            let requested_version = params
                .as_ref()
                .and_then(|p| p.get("protocolVersion"))
                .and_then(|v| v.as_str())
                .unwrap_or("2024-11-05");
            let protocol_version = if SUPPORTED_PROTOCOL_VERSIONS.contains(&requested_version) {
                requested_version
            } else {
                "2024-11-05"
            };

            if let Some(ref p) = params {
                state.set_roots_from_initialize(p);
                if let Some(name) = p.get("clientInfo").and_then(|c| c.get("name")).and_then(|n| n.as_str()) {
                    state.agent_client_name = Some(name.to_string());
                }
                if let Some(id) = p.get("sessionId")
                    .or_else(|| p.get("session_id"))
                    .or_else(|| p.get("conversationId"))
                    .or_else(|| p.get("conversation_id"))
                    .or_else(|| p.get("chatId"))
                    .or_else(|| p.get("chat_id"))
                    .or_else(|| p.get("clientInfo").and_then(|c| c.get("id")))
                    .and_then(|v| v.as_str())
                {
                    let clean_id = id.trim();
                    if !clean_id.is_empty() {
                        if !ServerState::is_valid_session_id(clean_id) {
                            return Err((
                                -32602,
                                format!(
                                    "Invalid session_id '{}': must contain only safe identifier characters without path separators or '..'.",
                                    clean_id
                                ),
                            ));
                        }
                        if state.session_id != clean_id {
                            switch_session_state(state, clean_id, None);
                        }
                    }
                }
            }
            Ok(json!({
                "protocolVersion": protocol_version,
                "capabilities": {
                    "tools": {},
                    "resources": {}
                },
                "serverInfo": {
                    "name": "Agent Guidance MCP Rust",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }))
        }
        "notifications/initialized" => Ok(json!({})),
        "notifications/cancelled" => {
            if let Some(ref flag) = state.cancellation {
                flag.store(true, Ordering::Relaxed);
            }
            Ok(json!({}))
        }
        "client/connect" => {
            if let Some(ref p) = params {
                if let Some(name) = p.get("name").and_then(|n| n.as_str()) {
                    state.agent_client_name = Some(name.to_string());
                }
            }
            Ok(json!({}))
        }
        "ping" => Ok(json!({})),
        "resources/list" => Ok(resources::handle_list()),
        "resources/templates/list" => Ok(resources::handle_templates_list()),
        "resources/read" => resources::handle_read(params, state),
        "tools/list" => Ok(tools::get_tools_list()),
        "tools/call" => {
            let params = params.ok_or((-32602, "Missing params".to_string()))?;
            let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("").trim();
            if name.is_empty() || !is_known_tool(name) {
                return Err((
                    -32601,
                    format!(
                        "Method not found: {}",
                        if name.is_empty() { "<empty>" } else { name }
                    ),
                ));
            }
            let arguments = match params.get("arguments") {
                None | Some(Value::Null) => json!({}),
                Some(v) if v.is_object() => v.clone(),
                Some(_) => {
                    return Err((
                        -32602,
                        "Invalid params: 'arguments' must be an object".to_string(),
                    ));
                }
            };

            let is_explicit_switch = name == "session_continuity"
                && arguments.get("operation").and_then(|o| o.as_str()) == Some("switch");
            if let Some(id) = arguments.get("conversation_id")
                .or_else(|| arguments.get("session_id"))
                .or_else(|| arguments.get("chat_id"))
                .and_then(|v| v.as_str())
            {
                let clean_id = id.trim();
                if !clean_id.is_empty() {
                    if !ServerState::is_valid_session_id(clean_id) {
                        return Err((
                            -32602,
                            format!(
                                "Invalid session_id '{}': must contain only safe identifier characters without path separators or '..'.",
                                clean_id
                            ),
                        ));
                    }
                    if !is_explicit_switch && state.session_id != clean_id {
                        let proj_hint = arguments.get("project_path").and_then(|v| v.as_str());
                        switch_session_state(state, clean_id, proj_hint);
                    }
                }
            }

            if let Err(err_msg) = state.can_call_tool(name, &arguments) {
                return Ok(json!({
                    "content": [
                        {
                            "type": "text",
                            "text": err_msg
                        }
                    ],
                    "isError": true
                }));
            }

            handle_tool_call(name, arguments, state)
        }
        _ => Err((-32601, format!("Method not found: {}", method))),
    }
}

#[allow(dead_code)]
pub fn is_read_only_request(method: &str, params: &Option<Value>) -> bool {
    match method {
        "notifications/initialized"
        | "ping"
        | "resources/list"
        | "resources/templates/list"
        | "resources/read"
        | "tools/list" => true,
        "initialize" | "client/connect" => false,
        "tools/call" => {
            if let Some(p) = params {
                let name = p.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let args = p.get("arguments");
                let has_session_override = args
                    .and_then(|a| {
                        a.get("conversation_id")
                            .or_else(|| a.get("session_id"))
                            .or_else(|| a.get("chat_id"))
                    })
                    .and_then(|v| v.as_str())
                    .map(|s| !s.trim().is_empty())
                    .unwrap_or(false);
                if has_session_override {
                    return false;
                }
                match name {
                    "project_context" => {
                        let authorize = args
                            .and_then(|a| a.get("authorize").or_else(|| a.get("pre_authorize")))
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        let op = args
                            .and_then(|a| a.get("operation"))
                            .and_then(|o| o.as_str())
                            .unwrap_or("tree");
                        !authorize
                            && !matches!(
                                op,
                                "read_and_authorize"
                                    | "read_auth"
                                    | "learn_alias"
                                    | "enrich_graph"
                                    | "enrich"
                                    | "reindex"
                                    | "architecture"
                            )
                    }
                    "guidance" => {
                        let op = args
                            .and_then(|a| a.get("operation"))
                            .and_then(|o| o.as_str())
                            .unwrap_or("list");
                        matches!(op, "list" | "get" | "docs" | "workflow" | "ui_ux")
                    }
                    "workflow_gate" => {
                        let action = args
                            .and_then(|a| a.get("action"))
                            .and_then(|act| act.as_str())
                            .unwrap_or("check");
                        matches!(action, "check" | "status")
                    }
                    "session_continuity" => {
                        let op = args
                            .and_then(|a| a.get("operation"))
                            .and_then(|o| o.as_str())
                            .unwrap_or("load");
                        matches!(op, "list" | "sessions" | "diff" | "changes")
                    }
                    _ => false,
                }
            } else {
                false
            }
        }
        _ => false,
    }
}

#[cfg(test)]
#[path = "../router_tests.rs"]
mod tests;
