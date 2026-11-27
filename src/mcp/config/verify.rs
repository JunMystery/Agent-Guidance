use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

use crate::mcp::templates::*;

use super::clients::check_mcp_registration_details;
use super::setup_targets::get_client_targets;

fn commands_match(configured: &str, expected: &Path) -> bool {
    let cfg_path = Path::new(configured.trim());
    match (cfg_path.canonicalize(), expected.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => {
            let norm_a = configured.trim().replace('\\', "/");
            let norm_b = expected.to_string_lossy().replace('\\', "/");
            if cfg!(windows) {
                norm_a.eq_ignore_ascii_case(&norm_b)
            } else {
                norm_a == norm_b
            }
        }
    }
}

pub fn run_verify_setup(binary_path: &Path) -> Result<()> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home dir"))?;
    let bin_str = binary_path.to_string_lossy().to_string();

    println!("=== Agent Guidance Setup Verification ===\n");

    // 1. Check binary
    let exists = binary_path.exists();
    println!(
        "[{}] Binary: {} ({})",
        if exists { "OK" } else { "FAIL" },
        bin_str,
        if exists { "found" } else { "NOT FOUND" }
    );

    // 2. Check MCP configs driven directly from setup_targets + OpenCode + Codex
    let (targets, extensions) = get_client_targets(&home);
    let mut mcp_targets: Vec<(&str, PathBuf, &str)> = targets
        .into_iter()
        .map(|t| (t.name, t.path, t.key))
        .collect();
    mcp_targets.push((
        "OpenCode",
        home.join(".config").join("opencode").join("opencode.json"),
        "mcp",
    ));
    mcp_targets.push((
        "ChatGPT / Codex",
        home.join(".codex").join("config.toml"),
        "mcp_servers",
    ));

    println!("\n--- MCP Client Registrations ---");
    for (name, path, key) in &mcp_targets {
        let (registered, cmd_opt) = check_mcp_registration_details(path, key);
        match (registered, cmd_opt) {
            (true, Some(cmd)) if !commands_match(&cmd, binary_path) => {
                println!(
                    "[WARN] {}: {} (configured command '{}' != expected '{}')",
                    name,
                    path.display(),
                    cmd,
                    bin_str
                );
            }
            (true, _) => {
                println!("[OK] {}: {}", name, path.display());
            }
            (false, _) => {
                println!("[  ] {}: {}", name, path.display());
            }
        }
    }

    for ext in &extensions {
        let has_host = ext
            .path
            .parent()
            .and_then(|p| p.parent())
            .is_some_and(|p| p.exists());
        let (registered, cmd_opt) = check_mcp_registration_details(&ext.path, "mcpServers");
        if has_host || registered {
            match (registered, cmd_opt) {
                (true, Some(cmd)) if !commands_match(&cmd, binary_path) => {
                    println!(
                        "[WARN] {}: {} (configured command '{}' != expected '{}')",
                        ext.name,
                        ext.path.display(),
                        cmd,
                        bin_str
                    );
                }
                (true, _) => println!("[OK] {}: {}", ext.name, ext.path.display()),
                (false, _) => println!("[  ] {}: {}", ext.name, ext.path.display()),
            }
        }
    }

    // 3. Check global rules (optional, not managed by --setup)
    println!("\n--- Global Rules (AGENTS.md / CLAUDE.md) [OPT: optional, not managed by --setup] ---");
    let rule_targets: Vec<(&str, PathBuf)> = vec![
        (
            "Gemini/Antigravity",
            home.join(".gemini").join("config").join("AGENTS.md"),
        ),
        (
            "OpenCode",
            home.join(".config").join("opencode").join("AGENTS.md"),
        ),
        ("Claude Code", home.join(".claude").join("CLAUDE.md")),
        ("ChatGPT/Codex", home.join(".codex").join("AGENTS.md")),
        (
            "Windsurf",
            home.join(".codeium").join("windsurf").join("AGENTS.md"),
        ),
    ];

    for (name, path) in &rule_targets {
        let has_tag = path.exists()
            && fs::read_to_string(path)
                .map(|c| c.contains(AGENT_GUIDANCE_TAG_START))
                .unwrap_or(false);
        println!("[{}] {}", if has_tag { "OK" } else { "OPT" }, name);
    }

    println!("\n=== Verification Complete ===");
    Ok(())
}
