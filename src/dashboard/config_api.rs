use serde::Deserialize;
use serde_json::json;
use std::env;
use tiny_http::Request;

use crate::client::RemoteMlClient;
use crate::config::{ConfigWatcher, ServerConfig, current_config};
use crate::mcp::config::run_setup;

#[derive(Deserialize)]
struct UpdateServerConfigRequest {
    #[serde(default)]
    url: String,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    clear_api_key: Option<bool>,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    dashboard_port: Option<u16>,
    #[serde(default)]
    dashboard_bind: Option<String>,
    #[serde(default)]
    auto_configure_ide: Option<bool>,
}

#[derive(Deserialize)]
struct TestConnectionRequest {
    url: String,
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

fn redacted_config(mut cfg: crate::config::AppConfig) -> (bool, crate::config::AppConfig) {
    let api_key_configured = !cfg.server.api_key.is_empty();
    cfg.server.api_key.clear();
    (api_key_configured, cfg)
}

fn is_valid_http_url(url: &str) -> bool {
    let t = url.trim();
    (t.starts_with("http://") || t.starts_with("https://"))
        && !t.chars().any(|c| c.is_whitespace() || c.is_control())
}

fn resolve_test_api_key(
    req_url: &str,
    req_key: Option<String>,
    cfg_server: &ServerConfig,
) -> String {
    if let Some(key) = req_key.filter(|k| !k.trim().is_empty()) {
        return key.trim().to_string();
    }
    if req_url.trim().trim_end_matches('/') == cfg_server.url.trim().trim_end_matches('/') {
        return cfg_server.api_key.clone();
    }
    String::new()
}

pub fn handle_api_server_config(mut request: Request) {
    match request.method() {
        tiny_http::Method::Get => {
            let (api_key_configured, safe_cfg) = redacted_config((*current_config()).clone());
            let payload = json!({
                "status": "ok",
                "api_key_configured": api_key_configured,
                "config": safe_cfg
            });
            super::router::json_response(request, 200, &payload);
        }
        tiny_http::Method::Post => {
            let body = match super::router::read_bounded_body(&mut request) {
                Ok(b) => b,
                Err((status, msg)) => {
                    let err = json!({ "status": "error", "message": msg });
                    super::router::json_response(request, status, &err);
                    return;
                }
            };

            let req_data: UpdateServerConfigRequest = match serde_json::from_str(&body) {
                Ok(data) => data,
                Err(e) => {
                    let err = json!({ "status": "error", "message": format!("Invalid JSON payload: {}", e) });
                    super::router::json_response(request, 400, &err);
                    return;
                }
            };

            if !req_data.url.trim().is_empty() && !is_valid_http_url(&req_data.url) {
                let err = json!({ "status": "error", "message": "Invalid server URL scheme (must be http:// or https://)" });
                super::router::json_response(request, 400, &err);
                return;
            }

            let mut cfg = (*current_config()).clone();
            if !req_data.url.is_empty() {
                cfg.server.url = req_data.url.trim().to_string();
            }
            if let Some(m) = req_data.mode {
                cfg.server.mode = m;
            }
            if req_data.clear_api_key == Some(true) {
                cfg.server.api_key.clear();
            } else if let Some(key) = req_data.api_key.filter(|k| !k.trim().is_empty()) {
                cfg.server.api_key = key.trim().to_string();
            }
            if let Some(t) = req_data.timeout_ms {
                cfg.server.timeout_ms = t;
            }
            if let Some(dp) = req_data.dashboard_port {
                cfg.dashboard.port = dp;
            }
            if let Some(db) = req_data.dashboard_bind {
                cfg.dashboard.bind = db;
            }

            if let Err(e) = ConfigWatcher::global().update(cfg.clone()) {
                let err = json!({ "status": "error", "message": format!("Failed to save config: {}", e) });
                super::router::json_response(request, 500, &err);
                return;
            }

            let mut ide_status = "Skipped".to_string();
            if req_data.auto_configure_ide.unwrap_or(true) {
                if let Ok(exe) = env::current_exe() {
                    match run_setup(&exe) {
                        Ok(_) => ide_status = "Updated across IDEs".to_string(),
                        Err(e) => ide_status = format!("Partial error: {}", e),
                    }
                }
            }

            let (api_key_configured, safe_cfg) = redacted_config(cfg);
            let payload = json!({
                "status": "ok",
                "message": "Configuration saved successfully",
                "ide_configuration": ide_status,
                "api_key_configured": api_key_configured,
                "config": safe_cfg
            });
            super::router::json_response(request, 200, &payload);
        }
        _ => {
            super::router::json_response(request, 405, &json!({ "error": "Method not allowed" }));
        }
    }
}

pub fn handle_api_test_connection(mut request: Request) {
    if request.method() != &tiny_http::Method::Post {
        super::router::json_response(request, 405, &json!({ "error": "POST required" }));
        return;
    }

    let body = match super::router::read_bounded_body(&mut request) {
        Ok(b) => b,
        Err((status, msg)) => {
            let err = json!({ "status": "error", "message": msg });
            super::router::json_response(request, status, &err);
            return;
        }
    };

    let req_data: TestConnectionRequest = match serde_json::from_str(&body) {
        Ok(data) => data,
        Err(e) => {
            let err = json!({ "status": "error", "message": format!("Invalid JSON: {}", e) });
            super::router::json_response(request, 400, &err);
            return;
        }
    };

    if !is_valid_http_url(&req_data.url) {
        let err = json!({ "status": "error", "message": "Invalid URL: must start with http:// or https://" });
        super::router::json_response(request, 400, &err);
        return;
    }

    let mut test_server = ServerConfig::default();
    test_server.url = req_data.url.trim().to_string();
    test_server.api_key = resolve_test_api_key(
        &req_data.url,
        req_data.api_key,
        &current_config().server,
    );
    if let Some(t) = req_data.timeout_ms {
        test_server.timeout_ms = t;
    }

    let client = RemoteMlClient::new(&test_server);
    match client.check_health() {
        Ok((health, latency)) => {
            let payload = json!({
                "status": "ok",
                "latency_ms": latency.as_millis(),
                "server_health": health
            });
            super::router::json_response(request, 200, &payload);
        }
        Err(e) => {
            let payload = json!({
                "status": "error",
                "message": format!("Failed to connect to server: {}", e)
            });
            super::router::json_response(request, 200, &payload);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_test_api_key_fallback() {
        let mut cfg = ServerConfig::default();
        cfg.url = "http://10.0.0.5:11998".to_string();
        cfg.api_key = "stored_secret".to_string();

        assert_eq!(resolve_test_api_key("http://10.0.0.5:11998/", None, &cfg), "stored_secret");
        assert_eq!(resolve_test_api_key("http://10.0.0.5:11998", Some("  ".into()), &cfg), "stored_secret");
        assert_eq!(resolve_test_api_key("http://10.0.0.5:11998", Some("override".into()), &cfg), "override");
        assert_eq!(resolve_test_api_key("http://other-host:11998", None, &cfg), "");
    }
}
