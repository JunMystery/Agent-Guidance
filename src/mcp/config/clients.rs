use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::info;

use crate::mcp::templates::*;

static CONFIG_TMP_SEQ: AtomicU64 = AtomicU64::new(0);

pub(crate) fn atomic_write_text(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let seq = CONFIG_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("config");
    let tmp_path = path.with_file_name(format!("{}.tmp.{}.{}", file_name, std::process::id(), seq));
    fs::write(&tmp_path, content)?;
    if let Err(e) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(e.into());
    }
    Ok(())
}

fn backup_existing_config(path: &Path, suffix: &str) {
    if path.exists() {
        let bak_path = path.with_extension(suffix);
        let _ = fs::copy(path, bak_path);
    }
}

pub(crate) fn parse_existing_json_object(config_path: &Path) -> Result<Value> {
    if !config_path.exists() {
        return Ok(json!({}));
    }
    let content = fs::read_to_string(config_path)?;
    if content.trim().is_empty() {
        return Ok(json!({}));
    }
    let root: Value = serde_json::from_str(&content).with_context(|| {
        format!(
            "Refusing to overwrite non-empty invalid JSON config at {}",
            config_path.display()
        )
    })?;
    if !root.is_object() {
        anyhow::bail!(
            "Refusing to overwrite non-object JSON config at {}",
            config_path.display()
        );
    }
    Ok(root)
}

pub(crate) fn check_mcp_registration_details(config_path: &Path, key: &str) -> (bool, Option<String>) {
    if !config_path.exists() {
        return (false, None);
    }
    let content = match fs::read_to_string(config_path) {
        Ok(c) => c,
        Err(_) => return (false, None),
    };

    let is_toml = config_path.extension().and_then(|e| e.to_str()) == Some("toml")
        || key == "mcp_servers";
    if is_toml {
        return match content.parse::<toml::Value>() {
            Ok(root) => {
                let entry = root
                    .get(key)
                    .and_then(|v| v.as_table())
                    .and_then(|m| m.get(SERVER_ID));
                let cmd = entry
                    .and_then(|e| e.get("command"))
                    .and_then(|c| c.as_str())
                    .map(|s| s.to_string());
                (entry.is_some(), cmd)
            }
            Err(_) => (false, None),
        };
    }

    match serde_json::from_str::<Value>(&content) {
        Ok(root) => {
            let entry = root
                .get(key)
                .and_then(|v| v.as_object())
                .and_then(|m| m.get(SERVER_ID));
            let cmd = entry.and_then(|e| {
                e.get("command").and_then(|c| {
                    c.as_str()
                        .map(|s| s.to_string())
                        .or_else(|| c.as_array().and_then(|a| a.first()).and_then(|v| v.as_str()).map(|s| s.to_string()))
                })
            });
            (entry.is_some(), cmd)
        }
        Err(_) => (false, None),
    }
}

#[allow(dead_code)]
pub(crate) fn check_mcp_registration(config_path: &Path, key: &str) -> bool {
    check_mcp_registration_details(config_path, key).0
}

// ---- Original MCP Config Functions (required by run_setup) ----

pub(crate) fn merge_mcp_config(config_path: &Path, server_id: &str, bin_path: &str, key: &str) -> Result<()> {
    let mut root = parse_existing_json_object(config_path)?;
    backup_existing_config(config_path, "json.bak");

    let obj = root
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("Root JSON must be an object"))?;
    if !obj.contains_key(key) {
        obj.insert(key.to_string(), json!({}));
    } else if !obj[key].is_object() {
        anyhow::bail!("Expected JSON object at key '{}' in {}", key, config_path.display());
    }

    let servers_map = obj
        .get_mut(key)
        .and_then(|v| v.as_object_mut())
        .ok_or_else(|| anyhow::anyhow!("Server entry must be an object"))?;
    servers_map.remove(crate::mcp::templates::OLD_SERVER_ID);

    let server_entry = if key == "servers" || config_path.to_string_lossy().ends_with(".claude.json") {
        json!({
            "type": "stdio",
            "command": bin_path,
            "args": []
        })
    } else {
        json!({
            "command": bin_path,
            "args": []
        })
    };

    servers_map.insert(server_id.to_string(), server_entry);
    atomic_write_text(config_path, &serde_json::to_string_pretty(&root)?)?;
    Ok(())
}

pub(crate) fn configure_opencode(opencode_path: &Path, bin_path: &str) -> Result<()> {
    let mut root = parse_existing_json_object(opencode_path)?;
    backup_existing_config(opencode_path, "json.bak");

    let obj = root
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("Root JSON must be an object"))?;
    if !obj.contains_key("mcp") {
        obj.insert("mcp".to_string(), json!({}));
    } else if !obj["mcp"].is_object() {
        anyhow::bail!("Expected JSON object at key 'mcp' in {}", opencode_path.display());
    }

    let mcp_map = obj
        .get_mut("mcp")
        .and_then(|v| v.as_object_mut())
        .ok_or_else(|| anyhow::anyhow!("mcp entry must be an object"))?;
    mcp_map.remove(crate::mcp::templates::OLD_SERVER_ID);
    mcp_map.insert(
        SERVER_ID.to_string(),
        json!({
            "type": "local",
            "command": [bin_path],
            "enabled": true,
            "environment": {}
        }),
    );

    atomic_write_text(opencode_path, &serde_json::to_string_pretty(&root)?)?;
    Ok(())
}

pub(crate) fn configure_codex_toml(codex_path: &Path, bin_path: &str) -> Result<()> {
    let mut value: toml::Value = if codex_path.exists() {
        let existing = fs::read_to_string(codex_path)?;
        if existing.trim().is_empty() {
            toml::Value::Table(toml::map::Map::new())
        } else {
            existing.parse::<toml::Value>().with_context(|| {
                format!(
                    "Refusing to overwrite invalid TOML config at {}",
                    codex_path.display()
                )
            })?
        }
    } else {
        toml::Value::Table(toml::map::Map::new())
    };
    backup_existing_config(codex_path, "toml.bak");

    let table = value
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("TOML root must be a table"))?;
    if !table.contains_key("mcp_servers") {
        table.insert(
            "mcp_servers".to_string(),
            toml::Value::Table(toml::map::Map::new()),
        );
    }

    let mcp_servers = table
        .get_mut("mcp_servers")
        .and_then(|v| v.as_table_mut())
        .ok_or_else(|| {
            anyhow::anyhow!("Expected TOML table at 'mcp_servers' in {}", codex_path.display())
        })?;

    mcp_servers.remove(crate::mcp::templates::OLD_SERVER_ID);

    mcp_servers.insert(
        SERVER_ID.to_string(),
        toml::Value::Table({
            let mut s = toml::map::Map::new();
            s.insert(
                "command".to_string(),
                toml::Value::String(bin_path.to_string()),
            );
            s.insert("args".to_string(), toml::Value::Array(vec![]));
            s
        }),
    );

    let toml_string = toml::to_string_pretty(&value)?;
    atomic_write_text(codex_path, &toml_string)?;
    info!("Successfully configured ChatGPT/Codex");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_mcp_registration_parses_toml_strictly() {
        let dir = std::env::temp_dir().join(format!("ag_clients_toml_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let toml_path = dir.join("config.toml");

        // Comment mentioning SERVER_ID must NOT register as present
        fs::write(&toml_path, "# agent-guidance comment only\n[other_section]\nname = \"agent-guidance\"\n").unwrap();
        assert!(!check_mcp_registration(&toml_path, "mcp_servers"));

        configure_codex_toml(&toml_path, "/usr/local/bin/agent-guidance").unwrap();
        let (reg, cmd) = check_mcp_registration_details(&toml_path, "mcp_servers");
        assert!(reg);
        assert_eq!(cmd.as_deref(), Some("/usr/local/bin/agent-guidance"));

        let _ = fs::remove_dir_all(&dir);
    }
}

