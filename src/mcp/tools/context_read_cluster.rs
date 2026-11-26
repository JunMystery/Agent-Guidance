use serde_json::Value;
use std::path::Path;

use crate::context::ast::{AstEngine, AstLanguage};
use crate::mcp::state::ServerState;
use super::helpers::{validate_path_scoped, TargetPathSpec};

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
    let mut valid_paths = Vec::new();

    let show_line_numbers = arguments
        .get("line_numbers")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let allow_cross = arguments
        .get("allow_cross_workspace")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let max_files = paths.len().min(8);
    for (idx, raw_spec) in paths.iter().take(max_files).enumerate() {
        let spec = TargetPathSpec::parse(raw_spec);
        if spec.clean_path.is_empty() {
            continue;
        }

        let full_path = match validate_path_scoped(proj_path, &spec.clean_path, allow_cross) {
            Ok(p) => p,
            Err(err_msg) => {
                sections.push(format!(
                    "### [{}/{}] File: `{}`\n\n**Security Error: PATH_TRAVERSAL_PROHIBITED**: {}",
                    idx + 1,
                    max_files,
                    spec.clean_path,
                    err_msg
                ));
                continue;
            }
        };

        if !full_path.exists() {
            valid_paths.push(spec.clean_path.clone());
            sections.push(format!(
                "### [{}/{}] File: `{}` [NEW FILE - Pending Creation]\n\n> [!NOTE] **New File Target**: Target file does not exist on disk yet. Path verified within workspace boundary.",
                idx + 1,
                max_files,
                spec.clean_path
            ));
            continue;
        }

        match std::fs::read_to_string(&full_path) {
            Ok(content) => {
                let file_tokens = crate::optimizer::compressor::estimate_tokens(&content, false) as u64;
                total_raw_tokens += file_tokens;

                let lines: Vec<&str> = content.lines().collect();
                let total_lines = lines.len();
                valid_paths.push(spec.clean_path.clone());

                let is_exempt = crate::mcp::tools::gate_edit::is_exempt_from_loc_limit(&spec.clean_path);
                let ext = Path::new(&spec.clean_path)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");
                let lang_str = detect_lang_tag(ext);

                // Cluster line budget guard: if total lines read > 1000, skeletonize remaining files
                let budget_exceeded = total_lines_read >= 1000;

                // 1. Line Range Slicing
                if let Some((start, end)) = spec.line_range {
                    if total_lines == 0 {
                        sections.push(format!(
                            "### [{}/{}] File: `{}` (Empty file, 0 lines)\n\n```{}\n\n```",
                            idx + 1, max_files, spec.clean_path, lang_str
                        ));
                    } else {
                        let s = start.max(1).min(total_lines);
                        let e = end.max(s).min(total_lines);
                        let slice = &lines[s - 1..e];
                        total_lines_read += slice.len();
                        let rendered = render_lines(slice, s, show_line_numbers || true);
                        sections.push(format!(
                            "### [{}/{}] File: `{}` (Lines {}-{} of {})\n\n```{}\n{}\n```",
                            idx + 1, max_files, spec.clean_path, s, e, total_lines, lang_str, rendered
                        ));
                    }
                // 2. Semantic Symbol Slicing
                } else if let Some(ref sym) = spec.target_symbol {
                    let ast_lang = AstLanguage::from_path(&spec.clean_path);
                    let mut sliced_lines = None;
                    if ast_lang.is_supported() {
                        if let Some(ast_sym) = AstEngine::find_symbol(&content, ast_lang, sym) {
                            if ast_sym.start_line > 0 && ast_sym.end_line <= total_lines && ast_sym.end_line >= ast_sym.start_line {
                                let s = ast_sym.start_line;
                                let e = ast_sym.end_line;
                                let slice = &lines[s - 1..e];
                                total_lines_read += slice.len();
                                let rendered = render_lines(slice, s, true);
                                sections.push(format!(
                                    "### [{}/{}] File: `{}` (Symbol: '{}' | Lines {}-{} of {})\n\n```{}\n{}\n```",
                                    idx + 1, max_files, spec.clean_path, sym, s, e, total_lines, lang_str, rendered
                                ));
                                sliced_lines = Some(());
                            }
                        }
                    }

                    if sliced_lines.is_none() {
                        if let Some(slice) = crate::optimizer::skeleton::generate_zoom_slice(&content, &spec.clean_path, sym) {
                            total_lines_read += 40;
                            sections.push(format!(
                                "### [{}/{}] File: `{}` (Zoom Read: '{}' | {} folded functions)\n\n```{}\n{}\n```",
                                idx + 1, max_files, spec.clean_path, sym, slice.folded_functions_count, lang_str, slice.sliced_content
                            ));
                        } else if total_lines > 300 && !is_exempt {
                            let skeleton = crate::optimizer::skeleton::generate_code_skeleton(&content, &spec.clean_path);
                            sections.push(format!(
                                "### [{}/{}] File: `{}` [Symbol '{}' not found -> Fallback to AST Skeleton]\n```{}\n{}\n```",
                                idx + 1, max_files, spec.clean_path, sym, lang_str, skeleton
                            ));
                        } else {
                            total_lines_read += total_lines;
                            let rendered = render_lines(&lines, 1, show_line_numbers);
                            sections.push(format!(
                                "### [{}/{}] File: `{}` (Lines 1-{} of {}) [Symbol '{}' not found]\n\n```{}\n{}\n```",
                                idx + 1, max_files, spec.clean_path, total_lines, total_lines, sym, lang_str, rendered
                            ));
                        }
                    }
                // 3. Fallback / Large File Skeleton
                } else if (total_lines > 300 || budget_exceeded) && !is_exempt {
                    let skeleton = crate::optimizer::skeleton::generate_code_skeleton(&content, &spec.clean_path);
                    let reason = if budget_exceeded { "Cluster Line Budget (1000 lines) Exceeded" } else { "Token Read Cap (> 300 LOC)" };
                    sections.push(format!(
                        "### [{}/{}] File: `{}`\n```{}\n{}\n```\n[ag: AST Structural Skeleton ({} lines) | {} | use #symbol or :start-end for specific section]",
                        idx + 1, max_files, spec.clean_path, lang_str, skeleton, total_lines, reason
                    ));
                } else {
                    total_lines_read += total_lines;
                    let rendered = render_lines(&lines, 1, show_line_numbers);
                    sections.push(format!(
                        "### [{}/{}] File: `{}` (Lines 1-{} of {})\n\n```{}\n{}\n```",
                        idx + 1, max_files, spec.clean_path, total_lines, total_lines, lang_str, rendered
                    ));
                }
            }
            Err(e) => {
                sections.push(format!(
                    "### [{}/{}] File: `{}`\n\nFailed to read file: {}",
                    idx + 1, max_files, spec.clean_path, e
                ));
            }
        }
    }

    state.last_raw_baseline_tokens = Some(total_raw_tokens);

    let auth_footer =
        super::helpers_auth::evaluate_read_authorization(arguments, proj_path, &valid_paths, state);

    format!(
        "# Multi-File Clustered Context ({} file(s), {} total lines read, ~{} baseline tokens)\n\n{}\n{}",
        sections.len(),
        total_lines_read,
        total_raw_tokens,
        sections.join("\n\n"),
        auth_footer
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
