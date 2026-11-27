use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::mcp::templates::*;

use super::clients::{atomic_write_text, parse_existing_json_object};
use super::rules_cleaner::{remove_global_rules, remove_skills_enforcer};

pub fn run_uninstall() -> Result<()> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home dir"))?;

    println!("Uninstalling agent-guidance from all IDE clients...");

    let (claude_path, code_path, _cursor_path, devin_path) = if cfg!(target_os = "windows") {
        let appdata = std::env::var("APPDATA").unwrap_or_default();
        let app_path = PathBuf::from(appdata);
        (
            app_path.join("Claude").join("claude_desktop_config.json"),
            app_path.join("Code").join("User").join("globalStorage"),
            app_path.join("Cursor").join("User").join("globalStorage"),
            app_path
                .join("Devin")
                .join("Cascade")
                .join("mcp_config.json"),
        )
    } else if cfg!(target_os = "macos") {
        (
            home.join("Library")
                .join("Application Support")
                .join("Claude")
                .join("claude_desktop_config.json"),
            home.join("Library")
                .join("Application Support")
                .join("Code")
                .join("User")
                .join("globalStorage"),
            home.join("Library")
                .join("Application Support")
                .join("Cursor")
                .join("User")
                .join("globalStorage"),
            home.join("Library")
                .join("Application Support")
                .join("Devin")
                .join("Cascade")
                .join("mcp_config.json"),
        )
    } else {
        (
            home.join(".config")
                .join("Claude")
                .join("claude_desktop_config.json"),
            home.join(".config")
                .join("Code")
                .join("User")
                .join("globalStorage"),
            home.join(".config")
                .join("Cursor")
                .join("User")
                .join("globalStorage"),
            home.join(".config")
                .join("Devin")
                .join("Cascade")
                .join("mcp_config.json"),
        )
    };

    let mcp_targets: Vec<(PathBuf, &str)> = vec![
        (claude_path, "mcpServers"),
        (
            home.join(".gemini").join("config").join("mcp_config.json"),
            "mcpServers",
        ),
        (
            home.join(".gemini")
                .join("antigravity")
                .join("mcp_config.json"),
            "mcpServers",
        ),
        (home.join(".cursor").join("mcp.json"), "mcpServers"),
        (
            code_path.parent().unwrap_or(&code_path).join("mcp.json"),
            "servers",
        ),
        (
            home.join(".continue")
                .join("mcpServers")
                .join("config.json"),
            "mcpServers",
        ),
        (devin_path, "mcpServers"),
        (home.join(".claude").join("mcp.json"), "mcpServers"),
        (
            home.join(".codeium")
                .join("windsurf")
                .join("mcp_config.json"),
            "mcpServers",
        ),
        (
            home.join(".config").join("opencode").join("opencode.json"),
            "mcp",
        ),
        (home.join(".codex").join("config.toml"), ""),
    ];

    for (path, key) in &mcp_targets {
        if path.exists() {
            let res = if !key.is_empty() {
                remove_mcp_entry(path, SERVER_ID, key)
            } else {
                remove_codex_entry(path)
            };
            match res {
                Ok(()) => println!("  Removed from: {}", path.display()),
                Err(e) => println!("  Skipped {} ({})", path.display(), e),
            }
        }
    }

    remove_global_rules(&home)?;
    remove_skills_enforcer(&home)?;

    println!("\nAgent Guidance uninstalled successfully.");
    Ok(())
}

fn remove_mcp_entry(config_path: &Path, server_id: &str, key: &str) -> Result<()> {
    let mut root = parse_existing_json_object(config_path)?;
    let _ = fs::copy(config_path, config_path.with_extension("json.bak"));
    if let Some(obj) = root.as_object_mut() {
        if let Some(servers) = obj.get_mut(key).and_then(|v| v.as_object_mut()) {
            servers.remove(server_id);
            servers.remove(crate::mcp::templates::OLD_SERVER_ID);
        }
    }
    atomic_write_text(config_path, &serde_json::to_string_pretty(&root)?)?;
    Ok(())
}

fn remove_codex_entry(config_path: &Path) -> Result<()> {
    let content = fs::read_to_string(config_path)?;
    if content.trim().is_empty() {
        return Ok(());
    }
    let mut value: toml::Value = content.parse::<toml::Value>().with_context(|| {
        format!(
            "Refusing to overwrite invalid TOML config at {}",
            config_path.display()
        )
    })?;
    let _ = fs::copy(config_path, config_path.with_extension("toml.bak"));
    if let Some(table) = value.as_table_mut() {
        if let Some(servers) = table.get_mut("mcp_servers").and_then(|v| v.as_table_mut()) {
            servers.remove(SERVER_ID);
            servers.remove(OLD_SERVER_ID);
        }
    }
    atomic_write_text(config_path, &toml::to_string_pretty(&value)?)?;
    Ok(())
}

