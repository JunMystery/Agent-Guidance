use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::mcp::templates::*;

use super::clients::{atomic_write_text, parse_existing_json_object};
use super::rules_cleaner::{remove_global_rules, remove_skills_enforcer};
use super::setup_targets::get_client_targets;

pub fn run_uninstall() -> Result<()> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home dir"))?;

    println!("Uninstalling agent-guidance from all IDE clients...");

    let (targets, extensions) = get_client_targets(&home);
    let mut mcp_targets: Vec<(PathBuf, &str)> = targets
        .into_iter()
        .map(|t| (t.path, t.key))
        .collect();

    for ext in extensions {
        mcp_targets.push((ext.path, "mcpServers"));
    }

    mcp_targets.push((
        home.join(".config").join("opencode").join("opencode.json"),
        "mcp",
    ));
    mcp_targets.push((home.join(".codex").join("config.toml"), ""));

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
