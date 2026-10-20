use rusqlite::{Connection, OpenFlags};
use serde_json::json;
use std::sync::{Arc, Mutex};
use tiny_http::Request;

use super::{StatsCache, json_response};

pub(crate) fn handle_api_dashboard_clean(mut request: Request, cache: &Arc<Mutex<StatsCache>>) {
    if request.method().as_str() != "POST" {
        json_response(request, 405, &json!({ "success": false, "error": "Method not allowed" }));
        return;
    }

    let mut content = String::new();
    let _ = request.as_reader().read_to_string(&mut content);
    let payload: serde_json::Value = serde_json::from_str(&content).unwrap_or(json!({}));
    let project_raw = payload
        .get("project")
        .and_then(|v| v.as_str())
        .unwrap_or("all");

    let project_filter = if project_raw.is_empty() || project_raw == "all" || project_raw == "." {
        None
    } else {
        Some(project_raw)
    };

    let db_path = crate::mcp::db::get_db_path();
    let conn = match Connection::open_with_flags(
        &db_path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_FULL_MUTEX,
    ) {
        Ok(c) => c,
        Err(e) => {
            json_response(request, 500, &json!({ "success": false, "error": e.to_string() }));
            return;
        }
    };

    match crate::mcp::db::clean_dashboard_history(&conn, project_filter) {
        Ok(cleaned) => {
            if let Ok(mut guard) = cache.lock() {
                guard.data = None;
            }
            json_response(
                request,
                200,
                &json!({
                    "success": true,
                    "cleaned_records": cleaned,
                    "project": project_raw,
                    "message": format!("Successfully cleaned {} dashboard metric record(s)", cleaned)
                }),
            );
        }
        Err(e) => json_response(request, 500, &json!({ "success": false, "error": e.to_string() })),
    }
}
