pub mod resources;
pub mod tools;

use crate::mcp::state::ServerState;
use crate::mcp::tools::handle_tool_call;
use serde_json::{Value, json};

pub fn handle_request(
    method: &str,
    params: Option<Value>,
    state: &mut ServerState,
) -> Result<Value, (i32, String)> {
    match method {
        "initialize" => {
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
                    if !clean_id.is_empty() && ServerState::is_valid_session_id(clean_id) {
                        state.session_id = clean_id.to_string();
                    }
                }
            }
            Ok(json!({
                "protocolVersion": "2024-11-05",
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
        "resources/read" => resources::handle_read(params, state),
        "tools/list" => Ok(tools::get_tools_list()),
        "tools/call" => {
            let params = params.ok_or((-32602, "Missing params".to_string()))?;
            let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

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
                        state.reset_for_context_switch();
                        state.session_id = clean_id.to_string();
                    }
                }
            }

            if let Err(err_msg) = state.can_call_tool(name, &arguments) {
                return Err((-32001, err_msg));
            }

            handle_tool_call(name, arguments, state)
        }
        _ => Err((-32601, format!("Method not found: {}", method))),
    }
}

pub fn is_read_only_request(method: &str, params: &Option<Value>) -> bool {
    match method {
        "notifications/initialized"
        | "ping"
        | "resources/list"
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
