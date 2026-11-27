use std::io::{Cursor, Read};
use tiny_http::{Header, Request, Response, StatusCode};

use super::handlers::*;
use super::state::WorkerState;

const MAX_WORKER_BODY_BYTES: u64 = 1024 * 1024;

fn allowed_cors_origin(request: &Request) -> Option<String> {
    let origin = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Origin"))
        .map(|h| h.value.as_str().trim().to_string())?;
    if crate::dashboard::router::is_loopback_origin(&origin) {
        Some(origin)
    } else {
        None
    }
}

pub fn route_request(mut request: Request, state: &WorkerState, api_key: &str) {
    let has_origin = request.headers().iter().any(|h| h.field.equiv("Origin"));
    let cors_origin = allowed_cors_origin(&request);

    if request.method().as_str() == "OPTIONS" {
        let status = if !has_origin || cors_origin.is_some() { 204 } else { 403 };
        let mut resp = Response::empty(status);
        add_cors_headers(&mut resp, cors_origin.as_deref(), "");
        let _ = request.respond(resp);
        return;
    }

    if has_origin && cors_origin.is_none() {
        send_error(request, 403, "Forbidden: cross-origin request rejected", "");
        return;
    }

    // Bearer token check
    if !api_key.is_empty() {
        let auth_hdr = request
            .headers()
            .iter()
            .find(|h| h.field.equiv("authorization"))
            .map(|h| h.value.as_str());

        let expected = format!("Bearer {}", api_key);
        if auth_hdr != Some(&expected) {
            let json = serde_json::json!({ "error": "Unauthorized: invalid or missing Bearer token" });
            send_json(request, 401, &json.to_string(), "");
            return;
        }
    }

    let url = request.url().to_string();
    let path = url.split('?').next().unwrap_or(&url);
    let method = request.method().as_str();

    let cat_hash = state
        .catalog
        .read()
        .map(|g| g.catalog_hash.clone())
        .unwrap_or_default();

    let if_none_match = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("if-none-match"))
        .map(|h| h.value.as_str().trim_matches('"'));

    let is_not_modified = !cat_hash.is_empty() && if_none_match == Some(cat_hash.as_str());

    match (method, path) {
        ("GET", "/health") => {
            let res = handle_health(state);
            send_json(request, 200, &serde_json::to_string(&res).unwrap_or_default(), &cat_hash);
        }
        ("GET", "/api/skills/stats") => {
            if is_not_modified {
                send_not_modified(request, &cat_hash);
                return;
            }
            let res = handle_stats(state);
            send_json(request, 200, &serde_json::to_string(&res).unwrap_or_default(), &cat_hash);
        }
        ("GET", "/api/skills/list") => {
            if is_not_modified {
                send_not_modified(request, &cat_hash);
                return;
            }
            let res = handle_list(state);
            send_json(request, 200, &serde_json::to_string(&res).unwrap_or_default(), &cat_hash);
        }
        ("POST", "/api/skills/search") => {
            let body = match read_body(&mut request) {
                Ok(b) => b,
                Err((status, msg)) => return send_error(request, status, &msg, &cat_hash),
            };
            match serde_json::from_str(&body).map_err(anyhow::Error::from).and_then(|r| handle_search(state, r)) {
                Ok(res) => send_json(request, 200, &serde_json::to_string(&res).unwrap_or_default(), &cat_hash),
                Err(e) => send_error(request, 400, &e.to_string(), &cat_hash),
            }
        }
        ("POST", "/api/skills/slice") => {
            let body = match read_body(&mut request) {
                Ok(b) => b,
                Err((status, msg)) => return send_error(request, status, &msg, &cat_hash),
            };
            match serde_json::from_str(&body).map_err(anyhow::Error::from).and_then(|r| handle_slice(state, r)) {
                Ok(res) => send_json(request, 200, &serde_json::to_string(&res).unwrap_or_default(), &cat_hash),
                Err(e) => send_error(request, 400, &e.to_string(), &cat_hash),
            }
        }
        ("POST", "/api/skills/delete") => {
            let body = match read_body(&mut request) {
                Ok(b) => b,
                Err((status, msg)) => return send_error(request, status, &msg, &cat_hash),
            };
            match serde_json::from_str(&body).map_err(anyhow::Error::from).and_then(|r| handle_delete(state, r)) {
                Ok(res) => send_json(request, 200, &serde_json::to_string(&res).unwrap_or_default(), &cat_hash),
                Err(e) => send_error(request, 500, &e.to_string(), &cat_hash),
            }
        }
        ("POST", "/api/skills/reindex") => {
            let body = match read_body(&mut request) {
                Ok(b) => b,
                Err((status, msg)) => return send_error(request, status, &msg, &cat_hash),
            };
            let req = serde_json::from_str(&body).unwrap_or(super::types::ReindexRequest { force: Some(false) });
            match handle_reindex(state, req) {
                Ok(res) => send_json(request, 200, &serde_json::to_string(&res).unwrap_or_default(), &cat_hash),
                Err(e) => send_error(request, 500, &e.to_string(), &cat_hash),
            }
        }
        ("POST", "/api/embed") => {
            let body = match read_body(&mut request) {
                Ok(b) => b,
                Err((status, msg)) => return send_error(request, status, &msg, &cat_hash),
            };
            match serde_json::from_str(&body).map_err(anyhow::Error::from).and_then(handle_embed) {
                Ok(res) => send_json(request, 200, &serde_json::to_string(&res).unwrap_or_default(), &cat_hash),
                Err(e) => send_error(request, 400, &e.to_string(), &cat_hash),
            }
        }
        _ => {
            send_error(request, 404, "Endpoint not found", &cat_hash);
        }
    }
}

fn read_body(req: &mut Request) -> Result<String, (u16, String)> {
    let mut buf = Vec::new();
    let mut reader = req.as_reader().take(MAX_WORKER_BODY_BYTES + 1);
    reader
        .read_to_end(&mut buf)
        .map_err(|e| (400, format!("Failed to read request body: {}", e)))?;
    if buf.len() as u64 > MAX_WORKER_BODY_BYTES {
        return Err((413, "Request body exceeds 1 MB limit".to_string()));
    }
    String::from_utf8(buf).map_err(|e| (400, format!("Invalid UTF-8 request body: {}", e)))
}

fn send_not_modified(req: Request, cat_hash: &str) {
    let cors_origin = allowed_cors_origin(&req);
    let mut resp = Response::empty(304);
    add_cors_headers(&mut resp, cors_origin.as_deref(), cat_hash);
    let _ = req.respond(resp);
}

fn send_json(req: Request, status: u16, json: &str, cat_hash: &str) {
    let cors_origin = allowed_cors_origin(&req);
    let mut resp = Response::new(
        StatusCode(status),
        vec![Header::from_bytes(&b"Content-Type"[..], &b"application/json; charset=utf-8"[..]).unwrap()],
        Cursor::new(json.as_bytes().to_vec()),
        Some(json.len()),
        None,
    );
    add_cors_headers(&mut resp, cors_origin.as_deref(), cat_hash);
    let _ = req.respond(resp);
}

fn send_error(req: Request, status: u16, msg: &str, cat_hash: &str) {
    let json = serde_json::json!({ "error": msg }).to_string();
    send_json(req, status, &json, cat_hash);
}

fn add_cors_headers<R: std::io::Read>(resp: &mut Response<R>, cors_origin: Option<&str>, cat_hash: &str) {
    if let Some(origin) = cors_origin {
        if let Ok(hdr) = Header::from_bytes(&b"Access-Control-Allow-Origin"[..], origin.as_bytes()) {
            resp.add_header(hdr);
            resp.add_header(Header::from_bytes(&b"Vary"[..], &b"Origin"[..]).unwrap());
            resp.add_header(Header::from_bytes(&b"Access-Control-Allow-Methods"[..], &b"GET, POST, OPTIONS"[..]).unwrap());
            resp.add_header(Header::from_bytes(&b"Access-Control-Allow-Headers"[..], &b"Content-Type, Authorization"[..]).unwrap());
        }
    }
    if !cat_hash.is_empty() {
        if let Ok(hdr) = Header::from_bytes(&b"ETag"[..], format!("\"{}\"", cat_hash).as_bytes()) {
            resp.add_header(hdr);
        }
    }
}
