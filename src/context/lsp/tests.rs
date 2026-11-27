//! Unit tests for LSP protocol framing, URI conversion, and server detection.

use super::*;
use serde_json::json;
use std::path::PathBuf;

#[test]
fn test_lsp_encode_decode_roundtrip() {
    let payload = json!({
        "jsonrpc": "2.0",
        "id": 42,
        "method": "textDocument/references",
        "params": { "context": { "includeDeclaration": true } }
    });

    let encoded = encode_message(&payload);
    assert!(encoded.starts_with(b"Content-Length: "));

    let (decoded, consumed) = decode_message(&encoded).expect("Should decode successfully");
    assert_eq!(consumed, encoded.len());
    assert_eq!(decoded["id"], 42);
    assert_eq!(decoded["method"], "textDocument/references");
}

#[test]
fn test_lsp_decode_incomplete_buffer() {
    let payload = json!({ "jsonrpc": "2.0", "id": 1 });
    let encoded = encode_message(&payload);

    // Truncated header
    assert_eq!(decode_message(&encoded[..10]), None);

    // Truncated body
    assert_eq!(decode_message(&encoded[..encoded.len() - 2]), None);
}

#[test]
fn test_lsp_position_and_line_conversion() {
    assert_eq!(line_to_lsp(1), 0);
    assert_eq!(line_to_lsp(50), 49);
    assert_eq!(line_to_lsp(0), 0);

    assert_eq!(line_from_lsp(0), 1);
    assert_eq!(line_from_lsp(49), 50);
}

#[test]
fn test_lsp_path_and_uri_conversion() {
    let temp_file = std::env::temp_dir().join("test_lsp_file.rs");
    let uri = path_to_uri(&temp_file);
    assert!(uri.starts_with("file:///"));

    let base = std::env::temp_dir();
    let rel = uri_to_rel_path(&uri, &base);
    assert_eq!(rel, "test_lsp_file.rs");
}

#[test]
fn test_detect_lsp_server_from_config() {
    let temp_dir = std::env::temp_dir().join(format!(
        "test_lsp_detect_cfg_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let _ = std::fs::create_dir_all(temp_dir.join(".agent-context"));

    let config_json = json!({
        "name": "custom-lsp",
        "command": if cfg!(windows) { "cmd" } else { "sh" },
        "args": ["--custom"]
    });

    std::fs::write(
        temp_dir.join(".agent-context").join("lsp.json"),
        config_json.to_string(),
    ).unwrap();

    let detected = detect_lsp_server(&temp_dir);
    assert!(detected.is_some());
    let cfg = detected.unwrap();
    assert_eq!(cfg.name, "custom-lsp");
    assert_eq!(cfg.args, vec!["--custom".to_string()]);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_graceful_fallback_when_server_missing() {
    let dummy_path = PathBuf::from("/nonexistent_directory_for_lsp_test");
    let result = query_lsp_references(&dummy_path, "src/main.rs", 10, 5);
    assert_eq!(result, None, "Must gracefully return None when LSP is unavailable");

    let def_result = query_lsp_definition(&dummy_path, "src/main.rs", 10, 5);
    assert_eq!(def_result, None, "Must gracefully return None when LSP is unavailable");
}

#[test]
fn test_handle_definition_fallback() {
    let temp_dir = std::env::temp_dir().join(format!(
        "test_lsp_def_fallback_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    let args = serde_json::json!({});
    let resp = crate::mcp::tools::context_lsp::handle_definition(&args, &temp_dir, "LspClient", "");
    assert!(resp.contains("Symbol Definition for 'LspClient'"));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_decode_message_overflow_and_case_insensitive_header() {
    let overflow_frame = b"Content-Length: 18446744073709551615\r\n\r\n{}";
    assert_eq!(decode_message(overflow_frame), None);

    let lower_header = b"content-length: 17\r\n\r\n{\"jsonrpc\":\"2.0\"}";
    let (decoded, consumed) = decode_message(lower_header).expect("Should decode lowercase header");
    assert_eq!(consumed, lower_header.len());
    assert_eq!(decoded["jsonrpc"], "2.0");
}

#[test]
fn test_path_to_uri_and_uri_to_rel_path_existing_file_and_percent_encoding() {
    let temp_dir = std::env::temp_dir().join(format!(
        "test_lsp_uri_space_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let sub_dir = temp_dir.join("my folder");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&sub_dir).unwrap();
    let file_path = sub_dir.join("hello world.rs");
    std::fs::write(&file_path, "fn main() {}\n").unwrap();

    let uri = path_to_uri(&file_path);
    assert!(uri.starts_with("file:///"));
    assert!(!uri.contains("/?/"), "URI must not leak Windows verbatim prefix: {}", uri);
    assert!(uri.contains("my%20folder/hello%20world.rs"), "URI must percent-encode spaces: {}", uri);

    let rel = uri_to_rel_path(&uri, &temp_dir);
    assert_eq!(rel, "my folder/hello world.rs");

    // Prefix boundary: sibling dir with shared prefix must not strip
    let sibling_dir = std::env::temp_dir().join(format!(
        "test_lsp_uri_space_{}_{:?}_other",
        std::process::id(),
        std::thread::current().id()
    ));
    let sibling_file = sibling_dir.join("other.rs");
    let sibling_uri = path_to_uri(&sibling_file);
    let non_rel = uri_to_rel_path(&sibling_uri, &temp_dir);
    assert_ne!(non_rel, "_other/other.rs");

    if cfg!(windows) {
        let rest = uri.strip_prefix("file:///").expect("file:/// prefix");
        let encoded_drive_uri = format!("file:///{}", rest.replacen(":/", "%3A/", 1));
        let rel_encoded = uri_to_rel_path(&encoded_drive_uri, &temp_dir);
        assert_eq!(rel_encoded, "my folder/hello world.rs");
    }

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_query_lsp_rejects_path_traversal_and_empty_command() {
    let temp_dir = std::env::temp_dir().join(format!(
        "test_lsp_traversal_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    assert_eq!(query_lsp_references(&temp_dir, "../secret.rs", 1, 1), None);
    assert_eq!(query_lsp_definition(&temp_dir, "/etc/passwd", 1, 1), None);
    assert_eq!(query_lsp_type_definition(&temp_dir, "src/\0main.rs", 1, 1), None);
    assert!(!detect::is_command_available(""));
    assert!(!detect::is_command_available("   "));
}
