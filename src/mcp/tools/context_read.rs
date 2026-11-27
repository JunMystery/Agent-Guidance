use serde_json::Value;
use std::path::{Path, PathBuf};

use crate::mcp::state::ServerState;
use super::helpers::{validate_path, validate_path_scoped};

pub(crate) struct ResolvedReadPath {
    pub full_path: PathBuf,
    pub resolved_subpath: String,
    pub auth_root: PathBuf,
    pub is_linked: bool,
}

pub(crate) enum ReadPathError {
    Security(String),
    LinkedNotFound(String),
}

impl ReadPathError {
    pub fn format_message(&self) -> String {
        match self {
            Self::Security(e) => format!("Security Error: {}", e),
            Self::LinkedNotFound(e) => format!("Error: {}", e),
        }
    }
}

pub(crate) fn resolve_read_target_path(
    proj_path: &Path,
    target_path: &str,
    allow_cross: bool,
) -> Result<ResolvedReadPath, ReadPathError> {
    if target_path.starts_with("linked:") {
        let linked = crate::context::multi_project::discover_linked_projects(proj_path);
        if let Some((proj, subpath)) =
            crate::context::multi_project::resolve_cross_project_path(target_path, &linked)
        {
            let full_path =
                validate_path(&proj.root_path, subpath).map_err(ReadPathError::Security)?;
            Ok(ResolvedReadPath {
                full_path,
                resolved_subpath: subpath.to_string(),
                auth_root: proj.root_path.clone(),
                is_linked: true,
            })
        } else {
            Err(ReadPathError::LinkedNotFound(format!(
                "Linked project not found for path '{}'. Ensure the project is registered in .agent-context/linked_projects.json or AGENT_GUIDANCE_LINKED_PROJECTS.",
                target_path
            )))
        }
    } else {
        let full_path = validate_path_scoped(proj_path, target_path, allow_cross)
            .map_err(ReadPathError::Security)?;
        Ok(ResolvedReadPath {
            full_path,
            resolved_subpath: target_path.to_string(),
            auth_root: proj_path.to_path_buf(),
            is_linked: false,
        })
    }
}

fn contains_ident_word(haystack: &str, ident: &str) -> Option<usize> {
    if ident.is_empty() {
        return None;
    }
    let is_ident_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let bytes = haystack.as_bytes();
    let mut start = 0;
    while let Some(pos) = haystack[start..].find(ident) {
        let abs = start + pos;
        let end = abs + ident.len();
        let left_ok = abs == 0 || !is_ident_char(bytes[abs - 1]);
        let right_ok = end == bytes.len() || !is_ident_char(bytes[end]);
        if left_ok && right_ok {
            return Some(abs);
        }
        start = abs + ident.len();
    }
    None
}

fn line_declares_symbol(line: &str, symbol: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("//")
        || trimmed.starts_with("/*")
        || trimmed.starts_with('*')
        || trimmed.starts_with('#')
        || trimmed.starts_with("--")
        || trimmed.starts_with("use ")
        || trimmed.starts_with("pub use ")
        || trimmed.starts_with("pub(crate) use ")
        || trimmed.starts_with("pub(super) use ")
        || trimmed.starts_with("import ")
        || trimmed.starts_with("from ")
        || trimmed.starts_with("package ")
        || trimmed.starts_with("using ")
    {
        return false;
    }

    let Some(pos) = contains_ident_word(trimmed, symbol) else {
        return false;
    };
    let prefix = trimmed[..pos].trim();
    let suffix = trimmed[pos + symbol.len()..].trim_start();
    let has_decl_kw = [
        "fn", "struct", "enum", "trait", "impl", "type", "const", "static", "mod",
        "def", "class", "fun", "func", "function", "interface", "protocol", "record",
        "object", "val", "var", "let", "export", "pub", "async", "suspend",
        "public", "private", "protected", "internal", "virtual", "override",
    ]
    .iter()
    .any(|kw| contains_ident_word(prefix, kw).is_some());

    if has_decl_kw {
        return true;
    }

    // Fallback for bare method/field definitions like `my_func(x) {` or `my_key:`
    prefix.is_empty() && (suffix.starts_with('(') || suffix.starts_with('<') || suffix.starts_with(':'))
}

pub(crate) fn handle_read(
    arguments: &Value,
    proj_path: &Path,
    rel_path: &str,
    state: &mut ServerState,
) -> String {
    let op = arguments.get("operation").and_then(|v| v.as_str()).unwrap_or("");
    let explicit_cluster = matches!(op, "cluster_read" | "read_cluster");
    let paths = crate::mcp::tools::gate_edit::extract_target_paths(arguments);
    if paths.len() > 1
        || (explicit_cluster && !paths.is_empty())
        || (paths.is_empty() && (arguments.get("relative_paths").is_some() || arguments.get("files").is_some()))
    {
        return super::context_read_cluster::handle_cluster_read(arguments, proj_path, &paths, state);
    }

    let raw_target = if !rel_path.is_empty() {
        rel_path
    } else if let Some(first) = paths.first() {
        first.as_str()
    } else {
        return "Error: relative_path or relative_paths is required for read operation. Example: project_context(operation=\"read\", project_path=\"...\", relative_paths=[\"src/a.rs\", \"src/b.rs\"])".to_string();
    };

    let spec = super::helpers::TargetPathSpec::parse(raw_target);
    let target_path = &spec.clean_path;

    let target_symbol = arguments
        .get("target_symbol")
        .and_then(|s| s.as_str())
        .or(spec.target_symbol.as_deref());
    let view_mode = arguments.get("view_mode").and_then(|v| v.as_str()).unwrap_or("auto");
    let start_line_arg = arguments
        .get("start_line")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .or_else(|| spec.line_range.map(|r| r.0));
    let end_line_arg = arguments
        .get("end_line")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .or_else(|| spec.line_range.map(|r| r.1));
    let show_line_numbers = arguments
        .get("line_numbers")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let allow_cross = arguments
        .get("allow_cross_workspace")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let resolved = match resolve_read_target_path(proj_path, target_path, allow_cross) {
        Ok(r) => r,
        Err(err) => return err.format_message(),
    };
    let full_path = &resolved.full_path;
    let resolved_subpath = &resolved.resolved_subpath;

    let authorize_requested = arguments
        .get("authorize")
        .or_else(|| arguments.get("pre_authorize"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let is_auth_op = op == "read_and_authorize" || op == "read_auth";

    let eval_auth = |st: &mut ServerState| {
        if resolved.is_linked && (authorize_requested || is_auth_op) {
            if !allow_cross {
                return format!(
                    "\n---\n# Edit Approval Gate Authorization\n\n- Status: BLOCKED (CROSS_WORKSPACE_WARNING)\n- Active Workspace: `{}`\n- Linked Project: `{}`\n- Target File: `{}`\n\n> [!WARNING] **Cross-Workspace Authorization Blocked**: Authorizing edits on a `linked:` cross-project file requires `allow_cross_workspace=true`.",
                    proj_path.display(),
                    resolved.auth_root.display(),
                    target_path
                );
            }
            if st.plan_approved {
                return super::helpers_auth::evaluate_read_authorization(
                    arguments,
                    &resolved.auth_root,
                    std::slice::from_ref(resolved_subpath),
                    st,
                );
            }
            let abs_target = full_path.to_string_lossy().replace('\\', "/");
            return super::helpers_auth::evaluate_read_authorization(
                arguments,
                proj_path,
                &[abs_target],
                st,
            );
        }
        super::helpers_auth::evaluate_read_authorization(
            arguments,
            &resolved.auth_root,
            std::slice::from_ref(resolved_subpath),
            st,
        )
    };

    if !full_path.exists() {
        let ext = Path::new(resolved_subpath)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let lang_tag = super::context_read_cluster::detect_lang_tag(ext);
        let auth_footer = eval_auth(state);
        return format!(
            "# Pending New File: {}\n\n```{}\n// [NEW FILE - Pending Creation]\n```\n> [!NOTE] This file does not exist yet on disk and will be created during edits.{}",
            resolved_subpath, lang_tag, auth_footer
        );
    }

    let cache_key = super::context_cache::AstCacheKey {
        path: full_path.to_string_lossy().replace('\\', "/"),
        target_symbol: target_symbol.map(|s| s.to_string()),
        view_mode: view_mode.to_string(),
        start_line: start_line_arg,
        end_line: end_line_arg,
        show_line_numbers,
    };

    if let Some((cached_body, age_secs, baseline_tokens)) = state.ast_cache.get(&cache_key, full_path) {
        state.last_raw_baseline_tokens = Some(baseline_tokens);
        let hit_notice = format!("\n[ag: AST Response Cache Hit (freshness: {}s/30s)]", age_secs);
        let auth_footer = eval_auth(state);
        return format!("{}{}{}", cached_body, hit_notice, auth_footer);
    }

    match std::fs::read_to_string(full_path) {
        Ok(content) => {
            let full_file_tokens = crate::optimizer::compressor::estimate_tokens(&content, false) as u64;
            state.last_raw_baseline_tokens = Some(full_file_tokens);
            let lines: Vec<&str> = content.lines().collect();
            let total_lines = lines.len();
            let is_exempt = crate::mcp::tools::gate_edit::is_exempt_from_loc_limit(resolved_subpath);

            // 1. If explicit skeleton requested, or code file is large (>300 LOC) and no target symbol/range requested
            let has_range = start_line_arg.is_some() || end_line_arg.is_some();
            if (view_mode == "skeleton" || (total_lines > 300 && target_symbol.is_none() && !has_range && view_mode != "full")) && !is_exempt {
                let skeleton = crate::optimizer::skeleton::generate_code_skeleton(&content, resolved_subpath);
                let body = format!(
                    "```\n{}\n```\n[ag: AST Structural Skeleton ({} lines) | Token Saver Mode: use target_symbol or line range for full body]",
                    skeleton,
                    total_lines
                );
                state.ast_cache.insert(cache_key, body.clone(), full_path, full_file_tokens);
                return format!("{}{}", body, eval_auth(state));
            }

            // 2. AST Semantic Context Slicing (Zoom Read)
            if let Some(symbol) = target_symbol.filter(|_| {
                view_mode == "zoom" || view_mode == "slice"
            }) {
                if let Some(slice) = crate::optimizer::skeleton::generate_zoom_slice(&content, resolved_subpath, symbol) {
                    let body = format!(
                        "```\n{}\n```\n[ag: Zoom Read (focus: {}) | {} folded functions, ~{}% token reduction]",
                        slice.sliced_content,
                        symbol,
                        slice.folded_functions_count,
                        slice.savings_percent
                    );
                    state.ast_cache.insert(cache_key, body.clone(), full_path, full_file_tokens);
                    return format!("{}{}", body, eval_auth(state));
                }
            }

            // 3. Fallback to isolated symbol snippet or bounded file content
            let mut display_lines = lines;
            let mut line_offset = 1;
            let mut symbol_found = false;
            let mut range_notice = String::new();
            let mut range_capped = false;

            if let Some(symbol) = target_symbol {
                let lang = crate::context::ast::AstLanguage::from_path(resolved_subpath);
                let mut ast_snippet = None;
                if lang.is_supported() {
                    if let Some(ast_sym) = crate::context::ast::AstEngine::find_symbol(&content, lang, symbol) {
                        if ast_sym.start_line > 0
                            && ast_sym.start_line <= total_lines
                            && ast_sym.end_line <= total_lines
                            && ast_sym.end_line >= ast_sym.start_line
                        {
                            line_offset = ast_sym.start_line;
                            ast_snippet = Some(display_lines[ast_sym.start_line - 1..ast_sym.end_line].to_vec());
                        }
                    }
                }

                if let Some(snippet) = ast_snippet {
                    display_lines = snippet;
                    symbol_found = true;
                } else {
                    // Heuristic declaration scanner (ignores use/import/comment lines)
                    let mut matched_snippet = Vec::new();
                    let mut capturing = false;
                    let mut brace_count = 0;
                    let mut has_braces = false;
                    let mut snippet_start = 1;
                    for (idx, line) in display_lines.iter().enumerate() {
                        if !capturing && line_declares_symbol(line, symbol) {
                            capturing = true;
                            snippet_start = idx + 1;
                        }
                        if capturing {
                            matched_snippet.push(*line);
                            let open_b = line.matches('{').count() as i32;
                            let close_b = line.matches('}').count() as i32;
                            if open_b > 0 {
                                has_braces = true;
                            }
                            brace_count += open_b - close_b;

                            if has_braces
                                && matched_snippet.len() > 1
                                && brace_count <= 0
                                && (line.contains('}') || line.trim().is_empty())
                            {
                                break;
                            }
                            if !has_braces
                                && matched_snippet.len() >= 30
                                && line.trim().is_empty()
                            {
                                break;
                            }
                            if matched_snippet.len() >= 100 {
                                break;
                            }
                        }
                    }
                    if !matched_snippet.is_empty() {
                        line_offset = snippet_start;
                        display_lines = matched_snippet;
                        symbol_found = true;
                    }
                }
            } else if has_range {
                if total_lines == 0 {
                    display_lines = Vec::new();
                } else {
                    let raw_start = start_line_arg.unwrap_or(1);
                    let raw_end = end_line_arg.unwrap_or(total_lines);
                    if raw_start == 0 || raw_start > total_lines || raw_end < raw_start {
                        range_notice = format!(
                            "\n\n> [!WARNING] Requested line range ({}-{}) is out of range for `{}` ({} total lines).",
                            raw_start, raw_end, target_path, total_lines
                        );
                        display_lines = Vec::new();
                    } else {
                        let start = raw_start.max(1);
                        let end = raw_end.min(total_lines);
                        line_offset = start;
                        let requested_len = end - start + 1;
                        let slice_len = requested_len.min(300);
                        if requested_len > 300 {
                            range_capped = true;
                            let shown_end = start + slice_len - 1;
                            range_notice.push_str(&format!(
                                "\n\n> [!NOTE] **Read Cap Applied (`was_capped=true`)**: Requested range {}-{} ({} lines) exceeded the 300-line cap; showing lines {}-{}. Continue with `start_line={}`.",
                                start, end, requested_len, start, shown_end, shown_end + 1
                            ));
                        } else if raw_end > total_lines {
                            range_notice.push_str(&format!(
                                "\n\n> [!NOTE] End line {} exceeded file length ({} lines); clamped to line {}.",
                                raw_end, total_lines, total_lines
                            ));
                        }
                        display_lines = display_lines[start - 1..(start - 1 + slice_len).min(total_lines)].to_vec();
                    }
                }
            }

            if let Some(symbol) = target_symbol {
                if !symbol_found && total_lines > 300 && !is_exempt {
                    let skeleton = crate::optimizer::skeleton::generate_code_skeleton(&content, resolved_subpath);
                    let body = format!(
                        "# Symbol Not Found: '{}' in {} ({} total lines)\n\n> [!WARNING] Target symbol `{}` was not found. Falling back to AST Structural Skeleton.\n\n```\n{}\n```",
                        symbol, target_path, total_lines, symbol, skeleton
                    );
                    state.ast_cache.insert(cache_key, body.clone(), full_path, full_file_tokens);
                    return format!("{}{}", body, eval_auth(state));
                }
            }

            let count = display_lines.len();
            let was_capped = range_capped || (!has_range && count > 300);

            let bounded = if show_line_numbers {
                display_lines
                    .into_iter()
                    .take(300)
                    .enumerate()
                    .map(|(i, line)| {
                        let line_no = line_offset + i;
                        format!("L{}: {}", line_no, line)
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                display_lines
                    .into_iter()
                    .take(300)
                    .collect::<Vec<_>>()
                    .join("\n")
            };

            let slice_end = if count == 0 { 0 } else { (line_offset + count.min(300)).saturating_sub(1) };
            let slice_start = if count == 0 { 0 } else { line_offset };

            let loc_warning = if was_capped && target_symbol.is_none() && !range_capped {
                if !is_exempt {
                    format!(
                        "\n\n---\n**Code Quality Note (Soft Limit > 300 LOC)**: File `{}` has **{} total lines** (read capped at 300 lines for token budget, `was_capped=true`). If editing, check whether it mixes unrelated domains or contains long/complex functions (> 60 LOC); keep intact if cohesive.",
                        target_path, total_lines
                    )
                } else {
                    format!(
                        "\n\n---\n> [!NOTE] **Read Cap Applied (`was_capped=true`)**: File `{}` has **{} total lines** (capped at 300 lines; pass `start_line`/`end_line` to read further).",
                        target_path, total_lines
                    )
                }
            } else {
                String::new()
            };

            let is_indent_sensitive = resolved_subpath.ends_with(".py")
                || resolved_subpath.ends_with(".yaml")
                || resolved_subpath.ends_with(".yml")
                || resolved_subpath.ends_with("Makefile")
                || resolved_subpath.ends_with(".mk")
                || resolved_subpath.ends_with(".nim");

            let indent_note = if is_indent_sensitive {
                "> **Language Note**: Indent-sensitive syntax (Python/YAML/Makefile). Whitespace and indentation preserved 100%.\n\n"
            } else {
                ""
            };

            let main_body = if let Some(symbol) = target_symbol {
                if symbol_found {
                    format!(
                        "# Target Symbol Extracted: '{}' from {} (Lines {}-{} of {})\n\n{}{}{}",
                        symbol, target_path, slice_start, slice_end, total_lines, indent_note, bounded, loc_warning
                    )
                } else {
                    format!(
                        "# Symbol Not Found: '{}' in {} (Showing Lines {}-{} of {})\n\n> [!WARNING] Target symbol `{}` was not found in `{}`.\n\n{}{}{}",
                        symbol, target_path, slice_start, slice_end, total_lines, symbol, target_path, indent_note, bounded, loc_warning
                    )
                }
            } else if total_lines == 0 {
                format!(
                    "# Bounded File Content: {} (Empty file, 0 lines)\n\n",
                    target_path
                )
            } else {
                format!(
                    "# Bounded File Content: {} (Lines {}-{} of {})\n\n{}{}{}{}",
                    target_path, slice_start, slice_end, total_lines, indent_note, bounded, range_notice, loc_warning
                )
            };
            state.ast_cache.insert(cache_key, main_body.clone(), full_path, full_file_tokens);
            let auth_footer = eval_auth(state);
            format!("{}{}", main_body, auth_footer)
        }
        Err(e) => format!("Failed to read file '{}': {}", target_path, e),
    }
}
