use serde::Deserialize;
use serde_json::json;
use tiny_http::Request;

use crate::catalog::staging::{get_skill_content, is_valid_skill_slug, purge_staging_skill, save_staging_skill};

#[derive(Deserialize)]
struct SaveSkillRequest {
    name: String,
    content: String,
}

#[derive(Deserialize)]
struct RevertSkillRequest {
    name: String,
}

pub fn handle_api_skills_content(request: Request) {
    let url = request.url();
    let query_param = url.split('?').nth(1).unwrap_or("");
    let mut skill_name = String::new();

    for pair in query_param.split('&') {
        let mut parts = pair.split('=');
        if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
            if k == "name" {
                skill_name = super::router::url_decode(v);
                break;
            }
        }
    }

    if !is_valid_skill_slug(&skill_name) {
        super::router::json_response(request, 400, &json!({ "error": "Valid query parameter 'name' is required" }));
        return;
    }

    match get_skill_content(&skill_name) {
        Some((content, source)) => {
            super::router::json_response(request, 200, &json!({
                "status": "ok",
                "name": skill_name,
                "content": content,
                "source": source
            }));
        }
        None => {
            super::router::json_response(request, 404, &json!({
                "error": format!("Skill '{}' not found", skill_name)
            }));
        }
    }
}

pub fn handle_api_skills_save(mut request: Request) {
    if request.method() != &tiny_http::Method::Post {
        super::router::json_response(request, 405, &json!({ "error": "POST required" }));
        return;
    }

    let body = match super::router::read_bounded_body(&mut request) {
        Ok(b) => b,
        Err((status, msg)) => {
            super::router::json_response(request, status, &json!({ "error": msg }));
            return;
        }
    };

    let req_data: SaveSkillRequest = match serde_json::from_str(&body) {
        Ok(d) => d,
        Err(e) => {
            super::router::json_response(request, 400, &json!({ "error": format!("Invalid JSON: {}", e) }));
            return;
        }
    };

    let trimmed = req_data.name.trim();
    if !is_valid_skill_slug(trimmed) {
        super::router::json_response(request, 400, &json!({ "error": "Invalid skill slug format" }));
        return;
    }

    match save_staging_skill(trimmed, &req_data.content) {
        Ok(()) => {
            super::router::json_response(request, 200, &json!({
                "status": "ok",
                "name": trimmed
            }));
        }
        Err(e) => {
            super::router::json_response(request, 500, &json!({ "error": e.to_string() }));
        }
    }
}

pub fn handle_api_skills_revert(mut request: Request) {
    if request.method() != &tiny_http::Method::Post {
        super::router::json_response(request, 405, &json!({ "error": "POST required" }));
        return;
    }

    let body = match super::router::read_bounded_body(&mut request) {
        Ok(b) => b,
        Err((status, msg)) => {
            super::router::json_response(request, status, &json!({ "error": msg }));
            return;
        }
    };

    let req_data: RevertSkillRequest = match serde_json::from_str(&body) {
        Ok(d) => d,
        Err(e) => {
            super::router::json_response(request, 400, &json!({ "error": format!("Invalid JSON: {}", e) }));
            return;
        }
    };

    let trimmed = req_data.name.trim();
    if !is_valid_skill_slug(trimmed) {
        super::router::json_response(request, 400, &json!({ "error": "Invalid skill slug format" }));
        return;
    }
    match purge_staging_skill(trimmed) {
        Ok(reverted) => {
            super::router::json_response(request, 200, &json!({
                "status": "ok",
                "name": trimmed,
                "reverted": reverted
            }));
        }
        Err(e) => {
            super::router::json_response(request, 500, &json!({ "error": e.to_string() }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_urlencoding_decode() {
        assert_eq!(super::super::router::url_decode("my%20skill"), "my skill");
        assert_eq!(super::super::router::url_decode("test+slug"), "test slug");
        assert_eq!(super::super::router::url_decode("codehealth-mcp"), "codehealth-mcp");
        assert_eq!(super::super::router::url_decode("t%E1%BA%A3o"), "tảo");
    }

    #[test]
    fn test_save_request_deserialization() {
        let json_str = r##"{"name":"test-skill","content":"# Heading\nContent"}"##;
        let req: SaveSkillRequest = serde_json::from_str(json_str).unwrap();
        assert_eq!(req.name, "test-skill");
        assert_eq!(req.content, "# Heading\nContent");
    }

    #[test]
    fn test_revert_request_deserialization() {
        let json_str = r#"{"name":"test-skill"}"#;
        let req: RevertSkillRequest = serde_json::from_str(json_str).unwrap();
        assert_eq!(req.name, "test-skill");
    }
}
