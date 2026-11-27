use serde_json::Value;
use std::path::{Path, PathBuf};

use crate::context::ast::{AstEngine, AstLanguage};
use crate::mcp::state::ServerState;
use super::context_read::resolve_read_target_path;
use super::helpers::TargetPathSpec;

pub(crate) fn handle_cluster_read(
    arguments: &Value,
    proj_path: &Path,
    paths: &[String],
    state: &mut ServerState,
) -> String {
    if paths.is_empty() {
        return "Error: No target files specified for clustered read.".to_string();
    }

    let mut sections = Vec::new();
    let mut total_lines_read = 0usize;
    let mut total_raw_tokens = 0u64;
    let mut valid_local_paths: Vec<String> = Vec::new();
    let mut linked_auth_targets: Vec<(PathBuf, String, String, String)> = Vec::new();

    let show_line_numbers = arguments
        .get("line_numbers")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let allow_cross = arguments
        .get("allow_cross_workspace")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let max_files = paths.len().min(8);
    let truncation_warning = if paths.len() > 8 {
        let omitted: Vec<&str> = paths.iter().skip(8).map(|s| s.as_str()).collect();
        format!(
            "> [!WARNING] **Cluster Limit Exceeded**: Requested {} files; capped at 8 files per clustered read. Omitted ({}): `{}`.\n\n",
            paths.len(),
            omitted.len(),
            omitted.join("`, `")
        )
    } else {
        String::new()
    };

    for (idx, raw_spec) in paths.iter().take(max_files).enumerate() {
        let spec = TargetPathSpec::parse(raw_spec);
        if spec.clean_path.is_empty() {
            continue;
        }

        let resolved = match resolve_read_target_path(proj_path, &spec.clean_path, allow_cross) {
            Ok(r) => r,
            Err(super::context_read::ReadPathError::Security(err_msg)) => {
                sections.push(format!(
                    "### [{}/{}] File: `{}`\n\n**Security Error: PATH_TRAVERSAL_PROHIBITED**: {}",
                    idx + 1,
                    max_files,
                    spec.clean_path,
                    err_msg
                ));
                continue;
            }
            Err(err) => {
                sections.push(format!(
                    "### [{}/{}] File: `{}`\n\n**{}**",
                    idx + 1,
                    max_files,
                    spec.clean_path,
                    err.format_message()
                ));
                continue;
            }
        };

        let full_path = &resolved.full_path;
        let display_name = &spec.clean_path;

        let mut record_valid = || {
            if resolved.is_linked {
                let abs = full_path.to_string_lossy().replace('\\', "/");
                linked_auth_targets.push((
                    resolved.auth_root.clone(),
                    resolved.resolved_subpath.clone(),
                    abs,
                    spec.clean_path.clone(),
                ));
            } else {
                valid_local_paths.push(resolved.resolved_subpath.clone());
            }
        };

        if !full_path.exists() {
            record_valid();
            sections.push(format!(
                "### [{}/{}] File: `{}` [NEW FILE - Pending Creation]\n\n> [!NOTE] **New File Target**: Target file does not exist on disk yet. Path verified within workspace boundary.",
                idx + 1,
                max_files,
                display_name
            ));
            continue;
        }

        match std::fs::read_to_string(full_path) {
            Ok(content) => {
                let file_tokens = crate::optimizer::compressor::estimate_tokens(&content, false) as u64;
                total_raw_tokens += file_tokens;

                let lines: Vec<&str> = content.lines().collect();
                let total_lines = lines.len();
                record_valid();

                let is_exempt = crate::mcp::tools::gate_edit::is_exempt_from_loc_limit(&resolved.resolved_subpath);
                let ext = Path::new(&resolved.resolved_subpath)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");
                let lang_str = detect_lang_tag(ext);

                // Cluster line budget guard: if total lines read > 1000, skeletonize/cap remaining files
                let budget_exceeded = total_lines_read >= 1000;

                // 1. Line Range Slicing (capped at 300 lines per file slice)
                if let Some((start, end)) = spec.line_range {
                    if total_lines == 0 {
                        sections.push(format!(
                            "### [{}/{}] File: `{}` (Empty file, 0 lines)\n\n```{}\n\n```",
                            idx + 1, max_files, display_name, lang_str
                        ));
                    } else if start == 0 || start > total_lines || end < start {
                        sections.push(format!(
                            "### [{}/{}] File: `{}`\n\n> [!WARNING] Requested line range ({}-{}) is out of range ({} total lines).",
                            idx + 1, max_files, display_name, start, end, total_lines
                        ));
                    } else {
                        let s = start.max(1).min(total_lines);
                        let raw_e = end.max(s).min(total_lines);
                        let e = raw_e.min(s + 299);
                        let slice = &lines[s - 1..e];
                        total_lines_read += slice.len();
                        let rendered = render_lines(slice, s, show_line_numbers);
                        let cap_note = if raw_e > e {
                            format!("\n[ag: Range capped at 300 lines (requested {}-{})]", s, raw_e)
                        } else {
                            String::new()
                        };
                        sections.push(format!(
                            "### [{}/{}] File: `{}` (Lines {}-{} of {})\n\n```{}\n{}\n```{}",
                            idx + 1, max_files, display_name, s, e, total_lines, lang_str, rendered, cap_note
                        ));
                    }
                // 2. Semantic Symbol Slicing
                } else if let Some(ref sym) = spec.target_symbol {
                    let ast_lang = AstLanguage::from_path(&resolved.resolved_subpath);
                    let mut sliced_lines = None;
                    if ast_lang.is_supported() {
                        if let Some(ast_sym) = AstEngine::find_symbol(&content, ast_lang, sym) {
                            if ast_sym.start_line > 0 && ast_sym.end_line <= total_lines && ast_sym.end_line >= ast_sym.start_line {
                                let s = ast_sym.start_line;
                                let e = ast_sym.end_line.min(s + 299);
                                let slice = &lines[s - 1..e];
                                total_lines_read += slice.len();
                                let rendered = render_lines(slice, s, true);
                                sections.push(format!(
                                    "### [{}/{}] File: `{}` (Symbol: '{}' | Lines {}-{} of {})\n\n```{}\n{}\n```",
                                    idx + 1, max_files, display_name, sym, s, e, total_lines, lang_str, rendered
                                ));
                                sliced_lines = Some(());
                            }
                        }
                    }

                    if sliced_lines.is_none() {
                        if let Some(slice) = crate::optimizer::skeleton::generate_zoom_slice(&content, &resolved.resolved_subpath, sym) {
                            total_lines_read += 40;
                            sections.push(format!(
                                "### [{}/{}] File: `{}` (Zoom Read: '{}' | {} folded functions)\n\n```{}\n{}\n```",
                                idx + 1, max_files, display_name, sym, slice.folded_functions_count, lang_str, slice.sliced_content
                            ));
                        } else if total_lines > 300 && !is_exempt {
                            let skeleton = crate::optimizer::skeleton::generate_code_skeleton(&content, &resolved.resolved_subpath);
                            sections.push(format!(
                                "### [{}/{}] File: `{}` [Symbol '{}' not found -> Fallback to AST Skeleton]\n```{}\n{}\n```",
                                idx + 1, max_files, display_name, sym, lang_str, skeleton
                            ));
                        } else {
                            let cap = total_lines.min(300);
                            total_lines_read += cap;
                            let rendered = render_lines(&lines[..cap], 1, show_line_numbers);
                            sections.push(format!(
                                "### [{}/{}] File: `{}` (Lines 1-{} of {}) [Symbol '{}' not found]\n\n```{}\n{}\n```",
                                idx + 1, max_files, display_name, cap, total_lines, sym, lang_str, rendered
                            ));
                        }
                    }
                // 3. Fallback / Large File Skeleton or Bounded Exempt Cap
                } else if (total_lines > 300 || budget_exceeded) && !is_exempt {
                    let skeleton = crate::optimizer::skeleton::generate_code_skeleton(&content, &resolved.resolved_subpath);
                    let reason = if budget_exceeded { "Cluster Line Budget (1000 lines) Exceeded" } else { "Token Read Cap (> 300 LOC)" };
                    sections.push(format!(
                        "### [{}/{}] File: `{}`\n```{}\n{}\n```\n[ag: AST Structural Skeleton ({} lines) | {} | use #symbol or :start-end for specific section]",
                        idx + 1, max_files, display_name, lang_str, skeleton, total_lines, reason
                    ));
                } else if (total_lines > 300 || budget_exceeded) && is_exempt {
                    let cap = if budget_exceeded { 60 } else { 300 }.min(total_lines);
                    total_lines_read += cap;
                    let rendered = render_lines(&lines[..cap], 1, show_line_numbers);
                    sections.push(format!(
                        "### [{}/{}] File: `{}` (Lines 1-{} of {})\n\n```{}\n{}\n```\n[ag: Exempt file capped at {} of {} lines | use :start-end for specific section]",
                        idx + 1, max_files, display_name, cap, total_lines, lang_str, rendered, cap, total_lines
                    ));
                } else {
                    total_lines_read += total_lines;
                    let rendered = render_lines(&lines, 1, show_line_numbers);
                    sections.push(format!(
                        "### [{}/{}] File: `{}` (Lines 1-{} of {})\n\n```{}\n{}\n```",
                        idx + 1, max_files, display_name, total_lines, total_lines, lang_str, rendered
                    ));
                }
            }
            Err(e) => {
                sections.push(format!(
                    "### [{}/{}] File: `{}`\n\nFailed to read file: {}",
                    idx + 1, max_files, display_name, e
                ));
            }
        }
    }

    state.last_raw_baseline_tokens = Some(total_raw_tokens);

    let authorize_requested = arguments
        .get("authorize")
        .or_else(|| arguments.get("pre_authorize"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let op = arguments.get("operation").and_then(|v| v.as_str()).unwrap_or("");
    let is_auth_op = op == "read_and_authorize" || op == "read_auth";

    let mut auth_footers = Vec::new();
    if !valid_local_paths.is_empty() {
        let local_footer = super::helpers_auth::evaluate_read_authorization(
            arguments,
            proj_path,
            &valid_local_paths,
            state,
        );
        if !local_footer.is_empty() {
            auth_footers.push(local_footer);
        }
    }
    if !linked_auth_targets.is_empty() && (authorize_requested || is_auth_op) {
        if !allow_cross {
            let names: Vec<&str> = linked_auth_targets.iter().map(|t| t.3.as_str()).collect();
            auth_footers.push(format!(
                "\n---\n# Edit Approval Gate Authorization\n\n- Status: BLOCKED (CROSS_WORKSPACE_WARNING)\n- Active Workspace: `{}`\n- Linked Targets: `{}`\n\n> [!WARNING] **Cross-Workspace Authorization Blocked**: Authorizing edits on `linked:` cross-project files requires `allow_cross_workspace=true`.",
                proj_path.display(),
                names.join(", ")
            ));
        } else {
            for (auth_root, subpath, abs_target, _) in &linked_auth_targets {
                let f = if state.plan_approved {
                    super::helpers_auth::evaluate_read_authorization(
                        arguments,
                        auth_root,
                        std::slice::from_ref(subpath),
                        state,
                    )
                } else {
                    super::helpers_auth::evaluate_read_authorization(
                        arguments,
                        auth_root,
                        std::slice::from_ref(abs_target),
                        state,
                    )
                };
                if !f.is_empty() {
                    auth_footers.push(f);
                }
            }
        }
    }

    format!(
        "# Multi-File Clustered Context ({} file(s), {} total lines read, ~{} baseline tokens)\n\n{}{}\n{}",
        sections.len(),
        total_lines_read,
        total_raw_tokens,
        truncation_warning,
        sections.join("\n\n"),
        auth_footers.join("\n")
    )
}

fn render_lines(lines: &[&str], line_offset: usize, show_line_numbers: bool) -> String {
    if show_line_numbers {
        lines
            .iter()
            .enumerate()
            .map(|(i, l)| format!("L{}: {}", line_offset + i, l))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        lines.join("\n")
    }
}

pub(crate) fn detect_lang_tag(ext: &str) -> &'static str {
    match ext {
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" => "javascript",
        "py" => "python",
        "go" => "go",
        "java" => "java",
        "cs" => "csharp",
        "kt" => "kotlin",
        "json" => "json",
        "toml" => "toml",
        "md" => "markdown",
        "yaml" | "yml" => "yaml",
        _ => "",
    }
}
