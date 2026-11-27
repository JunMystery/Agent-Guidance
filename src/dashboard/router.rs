use super::{DashboardAssets, StatsCache, graph, logs_api, projects, stats::handle_api_stats};
use serde_json::json;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tiny_http::{Header, Response, StatusCode};

pub(crate) const MAX_API_BODY_BYTES: u64 = 1024 * 1024;
static ENGINE_REFRESH_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

pub(crate) fn read_bounded_body(request: &mut tiny_http::Request) -> Result<String, (u16, String)> {
    let mut buf = Vec::new();
    let mut reader = request.as_reader().take(MAX_API_BODY_BYTES + 1);
    reader
        .read_to_end(&mut buf)
        .map_err(|e| (400, format!("Failed to read request body: {}", e)))?;
    if buf.len() as u64 > MAX_API_BODY_BYTES {
        return Err((413, "Request body exceeds 1 MB limit".to_string()));
    }
    String::from_utf8(buf).map_err(|e| (400, format!("Invalid UTF-8 request body: {}", e)))
}

pub(crate) fn is_loopback_origin(origin: &str) -> bool {
    let lower = origin.trim().to_ascii_lowercase();
    let rest = if let Some(r) = lower.strip_prefix("http://") {
        r
    } else if let Some(r) = lower.strip_prefix("https://") {
        r
    } else {
        return false;
    };
    let rest = rest.trim_end_matches('/');
    let host_port = if let Some(after_bracket) = rest.strip_prefix("[::1]") {
        after_bracket
    } else if let Some(after_host) = rest.strip_prefix("127.0.0.1") {
        after_host
    } else if let Some(after_host) = rest.strip_prefix("localhost") {
        after_host
    } else {
        return false;
    };
    host_port.is_empty()
        || host_port
            .strip_prefix(':')
            .is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

fn allowed_cors_origin(request: &tiny_http::Request) -> Option<String> {
    let origin = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Origin"))
        .map(|h| h.value.as_str().trim().to_string())?;
    if is_loopback_origin(&origin) {
        return Some(origin);
    }
    let host = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Host"))
        .map(|h| h.value.as_str().trim());
    if let Some(h) = host {
        let lower_origin = origin.trim_end_matches('/').to_ascii_lowercase();
        let lower_host = h.to_ascii_lowercase();
        if lower_origin == format!("http://{}", lower_host)
            || lower_origin == format!("https://{}", lower_host)
        {
            return Some(origin);
        }
    }
    None
}

fn check_dashboard_access(request: &tiny_http::Request, path: &str) -> Option<(u16, &'static str)> {
    if !path.starts_with("/api/") && path != "/health" {
        return None;
    }

    let has_origin = request.headers().iter().any(|h| h.field.equiv("Origin"));
    if has_origin && allowed_cors_origin(request).is_none() {
        return Some((403, "Forbidden: cross-origin request rejected"));
    }

    let is_remote_client = request
        .remote_addr()
        .map(|addr| !addr.ip().is_loopback())
        .unwrap_or(false);

    if is_remote_client && path.starts_with("/api/") {
        let cfg_key = crate::config::current_config().server.api_key.clone();
        let expected_key = if !cfg_key.is_empty() {
            cfg_key
        } else {
            std::env::var("AGENT_GUIDANCE_API_KEY").unwrap_or_default()
        };
        if expected_key.is_empty() {
            return Some((403, "Forbidden: remote API access requires configured api_key"));
        }
        let auth_hdr = request
            .headers()
            .iter()
            .find(|h| h.field.equiv("Authorization"))
            .map(|h| h.value.as_str());
        let expected = format!("Bearer {}", expected_key);
        if auth_hdr != Some(expected.as_str()) {
            return Some((401, "Unauthorized: invalid or missing Bearer token"));
        }
    }

    None
}

pub(crate) fn handle_dashboard_request(
    request: tiny_http::Request,
    project_path: &str,
    cache: &Arc<Mutex<StatsCache>>,
) {
    let url = request
        .url()
        .split('?')
        .next()
        .unwrap_or("/")
        .trim_end_matches('/');
    let path = if url.is_empty() { "/" } else { url };

    if request.method() == &tiny_http::Method::Options {
        let mut resp = Response::empty(204);
        if let Some(origin) = allowed_cors_origin(&request) {
            if let Ok(hdr) = Header::from_bytes(&b"Access-Control-Allow-Origin"[..], origin.as_bytes()) {
                resp.add_header(hdr);
            }
            resp.add_header(Header::from_bytes(&b"Vary"[..], &b"Origin"[..]).unwrap());
            resp.add_header(Header::from_bytes(&b"Access-Control-Allow-Methods"[..], &b"GET, POST, OPTIONS"[..]).unwrap());
            resp.add_header(Header::from_bytes(&b"Access-Control-Allow-Headers"[..], &b"Content-Type, Authorization"[..]).unwrap());
        }
        let _ = request.respond(resp);
        return;
    }

    if let Some((status, msg)) = check_dashboard_access(&request, path) {
        json_response(request, status, &json!({ "status": "error", "error": msg }));
        return;
    }

    match path {
        "/" | "/index.html" => serve_asset(request, "index.html", "text/html; charset=utf-8"),
        "/dashboard.css" => serve_asset(request, "dashboard.css", "text/css; charset=utf-8"),
        "/favicon.ico" | "/docs/images/logo.ico" => {
            let ico_bytes = include_bytes!("../../docs/images/logo.ico");
            let header = Header::from_bytes(&b"Content-Type"[..], &b"image/x-icon"[..]).unwrap();
            let _ = request.respond(Response::from_data(ico_bytes.as_slice()).with_header(header));
        }
        "/favicon.png" | "/logo.png" | "/docs/images/logo.png" => {
            let png_bytes = include_bytes!("../../docs/images/logo.png");
            let header = Header::from_bytes(&b"Content-Type"[..], &b"image/png"[..]).unwrap();
            let _ = request.respond(Response::from_data(png_bytes.as_slice()).with_header(header));
        }
        "/api/stats" => handle_api_stats(request, project_path, cache),
        "/api/engine/refresh" => handle_api_engine_refresh(request, cache),
        "/api/projects" => {
            let db_path = crate::mcp::db::get_db_path();
            projects::handle_api_projects(request, &db_path);
        }
        "/api/projects/prune" => {
            let db_path = crate::mcp::db::get_db_path();
            projects::handle_api_prune(request, &db_path);
        }
        "/api/projects/delete" => {
            let db_path = crate::mcp::db::get_db_path();
            projects::handle_api_delete_project(request, &db_path, cache);
        }
        "/api/graph" => graph::handle_api_graph(request, project_path),
        "/api/dashboard/clean" => super::dashboard_clean_api::handle_api_dashboard_clean(request, cache),
        "/api/cleanup" => handle_api_cleanup(request),
        "/api/skills/registry" => super::skills_api::handle_api_skills_registry(request),
        "/api/skills/binary_stats" => super::skills_api::handle_api_skills_binary_stats(request),
        "/api/skills/delete_bulk" => super::skills_api::handle_api_skills_delete_bulk(request),
        "/api/skills/compile" => super::skills_api::handle_api_skills_compile(request),
        "/api/skills/content" => super::skills_editor_api::handle_api_skills_content(request),
        "/api/skills/save" => super::skills_editor_api::handle_api_skills_save(request),
        "/api/skills/revert" => super::skills_editor_api::handle_api_skills_revert(request),
        "/api/config/server" => super::config_api::handle_api_server_config(request),
        "/api/config/test" => super::config_api::handle_api_test_connection(request),
        "/api/tool-rules" => super::rules_api::handle_api_tool_rules(request),
        "/api/tool-rules/revert" => super::rules_api::handle_api_tool_rules_revert(request),
        "/api/logs" | "/api/logs/clear" => logs_api::handle_api_logs(request),
        "/health" => {
            let db_bytes = crate::mcp::db::get_db_size_bytes();
            let json_data = json!({
                "status": "ok",
                "server": "agent-guidance-dashboard",
                "version": env!("CARGO_PKG_VERSION"),
                "model_loaded": true,
                "engine": "rust-candle",
                "backend": "candle-bert",
                "db_size_bytes": db_bytes,
                "clients": crate::daemon::active_clients_count()
            });
            json_response(request, 200, &json_data);
        }
        _ if path.starts_with("/js/") => {
            let rel = path.trim_start_matches("/js/");
            let asset_path = format!("js/{}", rel);
            serve_asset(
                request,
                &asset_path,
                "application/javascript; charset=utf-8",
            );
        }
        _ if path.starts_with("/css/") => {
            let rel = path.trim_start_matches("/css/");
            let asset_path = format!("css/{}", rel);
            serve_asset(
                request,
                &asset_path,
                "text/css; charset=utf-8",
            );
        }
        _ => json_response(request, 404, &json!({"error": "Not found"})),
    }
}

pub(crate) fn serve_asset(request: tiny_http::Request, name: &str, mime_type: &str) {
    if let Some(file) = DashboardAssets::get(name) {
        let data = file.data.into_owned();
        let header = Header::from_bytes(&b"Content-Type"[..], mime_type.as_bytes()).unwrap();
        let response = Response::from_data(data)
            .with_header(header)
            .with_status_code(StatusCode(200));
        let _ = request.respond(response);
    } else {
        json_response(
            request,
            404,
            &json!({"error": format!("Asset '{}' not found", name)}),
        );
    }
}

pub(crate) fn json_response(request: tiny_http::Request, status_code: u16, data: &serde_json::Value) {
    let body = serde_json::to_string(data).unwrap_or_default();
    let header_ct = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
    let cors_origin = allowed_cors_origin(&request);
    let mut response = Response::from_string(body)
        .with_header(header_ct)
        .with_status_code(StatusCode(status_code));
    if let Some(origin_str) = cors_origin {
        if let Ok(hdr) = Header::from_bytes(&b"Access-Control-Allow-Origin"[..], origin_str.as_bytes()) {
            response.add_header(hdr);
            response.add_header(Header::from_bytes(&b"Vary"[..], &b"Origin"[..]).unwrap());
            response.add_header(Header::from_bytes(&b"Access-Control-Allow-Methods"[..], &b"GET, POST, OPTIONS"[..]).unwrap());
            response.add_header(Header::from_bytes(&b"Access-Control-Allow-Headers"[..], &b"Content-Type, Authorization"[..]).unwrap());
        }
    }
    let _ = request.respond(response);
}

fn handle_api_cleanup(request: tiny_http::Request) {
    if request.method() != &tiny_http::Method::Post {
        json_response(request, 405, &json!({ "success": false, "error": "Method not allowed" }));
        return;
    }

    let db_path = crate::mcp::db::get_db_path();

    let conn = match rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_FULL_MUTEX,
    ) {
        Ok(c) => {
            let _ = c.busy_timeout(std::time::Duration::from_millis(2000));
            c
        }
        Err(e) => {
            json_response(request, 500, &json!({"error": e.to_string()}));
            return;
        }
    };

    match crate::mcp::db::run_auto_cleanup(&conn, crate::mcp::db::cleanup::DEFAULT_RETENTION_DAYS) {
        Ok(summary) => {
            let db_bytes = crate::mcp::db::get_db_size_bytes();
            json_response(
                request,
                200,
                &json!({
                    "success": true,
                    "summary": summary,
                    "db_size_bytes": db_bytes,
                    "db_size_mb": format!("{:.2} MB", db_bytes as f64 / (1024.0 * 1024.0))
                }),
            );
        }
        Err(e) => json_response(request, 500, &json!({"error": e.to_string()})),
    }
}

fn handle_api_engine_refresh(request: tiny_http::Request, cache: &Arc<Mutex<StatsCache>>) {
    if request.method() != &tiny_http::Method::Post {
        json_response(request, 405, &json!({ "success": false, "error": "Method not allowed" }));
        return;
    }

    crate::ml::embeddings::cache::clear_passage_cache();
    if let Ok(mut guard) = cache.lock() {
        guard.data = None;
    }
    if !ENGINE_REFRESH_IN_FLIGHT.swap(true, Ordering::SeqCst) {
        if std::thread::Builder::new()
            .name("ag-dash-refresh".to_string())
            .spawn(|| {
                crate::ml::embeddings::cache::warmup_cache();
                ENGINE_REFRESH_IN_FLIGHT.store(false, Ordering::SeqCst);
            })
            .is_err()
        {
            ENGINE_REFRESH_IN_FLIGHT.store(false, Ordering::SeqCst);
        }
    }
    let db_bytes = crate::mcp::db::get_db_size_bytes();
    json_response(
        request,
        200,
        &json!({
            "success": true,
            "message": "Neural vector embeddings cache refresh scheduled in background",
            "status": "ok",
            "engine": "rust-candle",
            "backend": "candle-bert",
            "model_loaded": true,
            "db_size_bytes": db_bytes,
            "clients": crate::daemon::active_clients_count()
        }),
    );
}

/// Decodes standard URL query string parameters, supporting percent-encoding (%XX),
/// '+' as space, and UTF-8 multi-byte sequences.
pub fn url_decode(s: &str) -> String {
    let mut bytes = Vec::with_capacity(s.len());
    let mut iter = s.as_bytes().iter().copied();
    while let Some(b) = iter.next() {
        if b == b'+' {
            bytes.push(b' ');
        } else if b == b'%' {
            let h1 = iter.next().unwrap_or(b'0');
            let h2 = iter.next().unwrap_or(b'0');
            let d1 = (h1 as char).to_digit(16);
            let d2 = (h2 as char).to_digit(16);
            if let (Some(n1), Some(n2)) = (d1, d2) {
                bytes.push(((n1 as u8) << 4) | (n2 as u8));
            } else {
                bytes.push(b'%');
                bytes.push(h1);
                bytes.push(h2);
            }
        } else {
            bytes.push(b);
        }
    }
    String::from_utf8(bytes).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_loopback_origin_validation() {
        assert!(is_loopback_origin("http://127.0.0.1:11997"));
        assert!(is_loopback_origin("http://localhost:3000"));
        assert!(is_loopback_origin("http://[::1]:11997"));
        assert!(is_loopback_origin("https://localhost"));
        assert!(!is_loopback_origin("http://localhost.evil.com"));
        assert!(!is_loopback_origin("https://evil.com"));
        assert!(!is_loopback_origin("null"));
    }

    #[test]
    fn test_url_decode_utf8_multibyte() {
        assert_eq!(url_decode("t%E1%BA%A3o+code"), "tảo code");
        assert_eq!(url_decode("agent-guidance"), "agent-guidance");
    }
}
