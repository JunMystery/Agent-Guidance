//! JSON-RPC 2.0 framing and protocol structures for Language Server Protocol (LSP).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

/// 0-indexed position in a text document per LSP specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspPosition {
    pub line: u32,
    pub character: u32,
}

/// A range in a text document expressed as start and end positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspRange {
    pub start: LspPosition,
    pub end: LspPosition,
}

/// Represents a location inside a resource, such as a line range inside a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspLocation {
    pub uri: String,
    pub range: LspRange,
}

/// Encodes a JSON-RPC payload with LSP `Content-Length` header.
pub fn encode_message(payload: &Value) -> Vec<u8> {
    let body = payload.to_string();
    format!("Content-Length: {}\r\n\r\n{}", body.len(), body).into_bytes()
}

/// Attempts to decode one framed JSON-RPC message from a byte buffer.
/// Returns `Some((Value, bytes_consumed))` if a complete message is present.
pub fn decode_message(buf: &[u8]) -> Option<(Value, usize)> {
    let header_sep = b"\r\n\r\n";
    let sep_pos = buf.windows(4).position(|w| w == header_sep)?;
    let header_bytes = &buf[..sep_pos];
    let header_str = std::str::from_utf8(header_bytes).ok()?;

    let mut content_length = None;
    for line in header_str.lines() {
        if let Some((name, val)) = line.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
            && let Ok(len) = val.trim().parse::<usize>()
        {
            content_length = Some(len);
            break;
        }
    }

    let len = content_length?;
    let body_start = sep_pos + 4;
    let body_end = body_start.checked_add(len)?;

    if buf.len() < body_end {
        return None; // Incomplete body
    }

    let body_bytes = &buf[body_start..body_end];
    let val = serde_json::from_slice::<Value>(body_bytes).ok()?;
    Some((val, body_end))
}

fn strip_windows_verbatim(path: &str) -> String {
    if let Some(unc) = path
        .strip_prefix("//?/UNC/")
        .or_else(|| path.strip_prefix("//?/unc/"))
    {
        format!("//{}", unc)
    } else if let Some(rest) = path.strip_prefix("//?/") {
        rest.to_string()
    } else {
        path.to_string()
    }
}

fn percent_encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for &b in path.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'/' | b':' | b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2]))
        {
            out.push((hi << 4) | lo);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

fn normalize_fs_path(path: &Path) -> String {
    strip_windows_verbatim(&path.to_string_lossy().replace('\\', "/"))
}

fn strip_path_prefix(target: &str, base: &str) -> Option<String> {
    if base == "/" {
        return target.strip_prefix('/').map(str::to_string);
    }
    let base_trimmed = base.trim_end_matches('/');
    if base_trimmed.is_empty() {
        return None;
    }
    let matches_prefix = if cfg!(windows) {
        target
            .get(..base_trimmed.len())
            .map(|prefix| prefix.eq_ignore_ascii_case(base_trimmed))
            .unwrap_or(false)
    } else {
        target.starts_with(base_trimmed)
    };
    if !matches_prefix {
        return None;
    }
    let rest = &target[base_trimmed.len()..];
    if rest.is_empty() {
        Some(String::new())
    } else {
        rest.strip_prefix('/').map(str::to_string)
    }
}

/// Converts a standard file path to an LSP file URI (`file:///...`).
pub fn path_to_uri(path: &Path) -> String {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let normalized = normalize_fs_path(&canonical);
    let encoded = percent_encode_path(normalized.trim_start_matches('/'));
    format!("file:///{}", encoded)
}

/// Converts an LSP file URI back to a workspace relative path.
pub fn uri_to_rel_path(uri: &str, proj_path: &Path) -> String {
    let raw = uri
        .strip_prefix("file://localhost/")
        .or_else(|| uri.strip_prefix("file:///"))
        .unwrap_or(uri);
    let decoded = strip_windows_verbatim(&percent_decode(raw).replace('\\', "/"));
    let target_norm = if cfg!(windows) {
        let trimmed = decoded.strip_prefix('/').unwrap_or(&decoded);
        if trimmed.as_bytes().get(1) == Some(&b':') {
            trimmed.to_string()
        } else {
            decoded
        }
    } else if !decoded.starts_with('/') && uri.starts_with("file://") {
        format!("/{}", decoded)
    } else {
        decoded
    };

    let mut bases = vec![normalize_fs_path(proj_path)];
    if let Ok(canon_proj) = proj_path.canonicalize() {
        let canon_norm = normalize_fs_path(&canon_proj);
        if !bases.contains(&canon_norm) {
            bases.push(canon_norm);
        }
    }

    for base in &bases {
        if let Some(rel) = strip_path_prefix(&target_norm, base) {
            return rel;
        }
    }

    target_norm
}

/// Converts a 1-based editor line number to a 0-based LSP line number.
pub fn line_to_lsp(line: usize) -> u32 {
    line.saturating_sub(1) as u32
}

/// Converts a 0-based LSP line number to a 1-based editor line number.
pub fn line_from_lsp(lsp_line: u32) -> usize {
    lsp_line as usize + 1
}
