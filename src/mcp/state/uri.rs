/// Decode RFC 3986 percent-encoded UTF-8 bytes, stripping null bytes.
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2]))
        {
            let decoded = (hi << 4) | lo;
            if decoded != 0 {
                out.push(decoded);
            }
            i += 3;
            continue;
        }
        if bytes[i] != 0 {
            out.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Parse cross-platform file:// URIs safely into native OS path strings.
pub fn parse_file_uri(uri: &str) -> String {
    let mut decoded = percent_decode(uri);
    if decoded
        .as_bytes()
        .get(..7)
        .is_some_and(|b| b.eq_ignore_ascii_case(b"file://"))
    {
        decoded = decoded[7..].to_string();
        if decoded
            .as_bytes()
            .get(..9)
            .is_some_and(|b| b.eq_ignore_ascii_case(b"localhost"))
            && decoded.as_bytes().get(9) == Some(&b'/')
        {
            decoded = decoded[9..].to_string();
        }
    }

    if cfg!(windows) {
        // Handle Windows leading slash e.g. /C:/path or /e:/path -> C:\path or E:\path
        let b = decoded.as_bytes();
        if b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':' {
            decoded = decoded[1..].to_string();
        }
        decoded = decoded.replace('/', "\\");
        let bytes = decoded.as_bytes();
        if bytes.len() >= 2 && bytes[0].is_ascii_lowercase() && bytes[1] == b':' {
            let upper = (bytes[0].to_ascii_uppercase() as char).to_string();
            decoded.replace_range(..1, &upper);
        }
    }

    decoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_file_uri_utf8_localhost_and_null_strip() {
        let utf8 = parse_file_uri("file:///C:/Users/D%E1%BB%B1%20%C3%A1n/my%23repo");
        if cfg!(windows) {
            assert_eq!(utf8, "C:\\Users\\Dự án\\my#repo");
        } else {
            assert_eq!(utf8, "/C:/Users/Dự án/my#repo");
        }

        let localhost = parse_file_uri("file://localhost/e:/work%20dir/app");
        if cfg!(windows) {
            assert_eq!(localhost, "E:\\work dir\\app");
        } else {
            assert_eq!(localhost, "/e:/work dir/app");
        }

        let nul = parse_file_uri("file:///home/user/a%00b");
        assert!(!nul.contains('\0'));

        // Multi-byte UTF-8 raw path must not panic on prefix slicing
        let non_uri_utf8 = parse_file_uri("đường_dẫn/thư_mục");
        assert!(!non_uri_utf8.is_empty());
    }
}
