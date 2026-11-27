use regex::Regex;
use std::sync::OnceLock;

static RE_COMMENTS: OnceLock<Regex> = OnceLock::new();
static RE_BADGES: OnceLock<Regex> = OnceLock::new();

pub fn estimate_tokens(text: &str, is_code: bool) -> usize {
    let ratio = if is_code { 2.8 } else { 4.0 };
    ((text.len() as f64) / ratio).ceil() as usize
}

/// Tracks CommonMark fenced code block state across lines.
/// Returns `true` if `line` opens or closes a fenced code block.
pub(crate) fn update_code_fence(line: &str, open_fence: &mut Option<(char, usize)>) -> bool {
    let trimmed = line.trim_start();
    let first = match trimmed.chars().next() {
        Some(c @ ('`' | '~')) => c,
        _ => return false,
    };
    let count = trimmed.chars().take_while(|&c| c == first).count();
    if count < 3 {
        return false;
    }
    let rest = &trimmed[count..];
    match *open_fence {
        None => {
            if first == '`' && rest.contains('`') {
                return false;
            }
            *open_fence = Some((first, count));
            true
        }
        Some((fence_char, fence_len)) => {
            if first == fence_char && count >= fence_len && rest.trim().is_empty() {
                *open_fence = None;
                true
            } else {
                false
            }
        }
    }
}

fn strip_markdown_noise(content: &str) -> String {
    let has_comments = content.contains("<!--");
    let has_badges = content.contains("shields.io") || content.contains("badge");

    let stripped = if has_comments {
        let re_comments = RE_COMMENTS.get_or_init(|| {
            Regex::new(r"(?s)<!--.*?-->").expect("Invalid comment regex")
        });
        re_comments.replace_all(content, "")
    } else {
        std::borrow::Cow::Borrowed(content)
    };

    if has_badges {
        let re_badges = RE_BADGES.get_or_init(|| {
            Regex::new(r"!\[[^\]]*\]\(https://img\.shields\.io/[^)]*\)[ \t]*|!\[[^\]]*\]\(https://badge[^)]*\)[ \t]*")
                .expect("Invalid badge regex")
        });
        re_badges.replace_all(&stripped, "").into_owned()
    } else {
        stripped.into_owned()
    }
}

fn push_compressed_markdown_chunk(chunk: &str, out: &mut Vec<String>) {
    if chunk.is_empty() {
        return;
    }
    let cleaned = strip_markdown_noise(chunk);
    let mut blank_count = 0;
    for line in cleaned.lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            blank_count += 1;
            if blank_count <= 1 {
                out.push(String::new());
            }
        } else {
            blank_count = 0;
            out.push(trimmed.to_string());
        }
    }
}

pub fn compress_markdown(content: &str) -> String {
    let mut out = Vec::new();
    let mut md_buf = String::new();
    let mut open_fence: Option<(char, usize)> = None;

    for line in content.lines() {
        let was_in_code = open_fence.is_some();
        if update_code_fence(line, &mut open_fence) {
            if !was_in_code {
                push_compressed_markdown_chunk(&md_buf, &mut out);
                md_buf.clear();
            }
            out.push(line.trim_end().to_string());
            continue;
        }
        if open_fence.is_some() {
            out.push(line.to_string());
        } else {
            md_buf.push_str(line);
            md_buf.push('\n');
        }
    }
    if open_fence.is_none() && !md_buf.is_empty() {
        push_compressed_markdown_chunk(&md_buf, &mut out);
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens() {
        let text = "Hello world";
        assert!(estimate_tokens(text, false) > 0);
        assert!(estimate_tokens(text, true) > 0);
    }

    #[test]
    fn test_compress_markdown() {
        let md = "# Title\n\n\n\n\n\nBody text";
        let compressed = compress_markdown(md);
        assert!(!compressed.contains("comment"));
        assert!(!compressed.contains("shields.io"));
        assert!(compressed.contains("# Title"));
        assert!(compressed.contains("Body text"));
    }

    #[test]
    fn test_compress_markdown_preserves_code_blocks() {
        let md = "# Title\n\n```rust\nfn foo() {\n\n\n    let x = 1;\n}\n```\n\n\n\nTrailing";
        let compressed = compress_markdown(md);
        assert!(compressed.contains("fn foo() {\n\n\n    let x = 1;\n}"));
        assert!(!compressed.contains("\n\n\nTrailing"));
    }

    #[test]
    fn test_compress_markdown_preserves_html_comments_in_code_blocks() {
        let md = "# Doc\n\n<!-- outer comment -->\n\n```html\n<div><!-- keep me --></div>\n![b](https://img.shields.io/badge/a-b)\n```\n";
        let compressed = compress_markdown(md);
        assert!(!compressed.contains("outer comment"));
        assert!(compressed.contains("<div><!-- keep me --></div>"));
        assert!(compressed.contains("https://img.shields.io/badge/a-b"));
    }

    #[test]
    fn test_compress_markdown_badge_preserves_preceding_image_and_heading() {
        let md = "![Arch](https://example.com/arch.png) ![CI](https://img.shields.io/badge/ci-ok)\nStatus: ![Ver](https://badge.fury.io/js/x.svg)\n# Next Heading\n";
        let compressed = compress_markdown(md);
        assert!(compressed.contains("![Arch](https://example.com/arch.png)"), "non-badge image stripped: {}", compressed);
        assert!(!compressed.contains("shields.io"));
        assert!(!compressed.contains("badge.fury.io"));
        assert!(compressed.contains("\n# Next Heading"), "heading merged onto prior line: {}", compressed);
    }

    #[test]
    fn test_compress_markdown_indented_nested_and_inline_fences() {
        let md = "\
```inline``` text

<!-- drop this -->

- Item:
  ```html
  <!-- keep indented -->


  <p>ok</p>
  ```

````markdown
```rust
<!-- keep nested -->
```
````

<!-- drop trailing -->
End";
        let compressed = compress_markdown(md);
        assert!(!compressed.contains("drop this"), "inline triple-backtick broke fence state: {}", compressed);
        assert!(compressed.contains("<!-- keep indented -->"), "indented code fence stripped: {}", compressed);
        assert!(compressed.contains("<!-- keep nested -->"), "nested 4-backtick fence broken: {}", compressed);
        assert!(!compressed.contains("drop trailing"), "trailing comment not stripped: {}", compressed);
        assert!(compressed.contains("End"));
    }
}
