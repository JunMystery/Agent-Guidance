use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use super::schema::ToolRulesConfig;

pub const TOOL_RULES_FILE: &str = "tool_rules.toml";
pub const AGENT_CONTEXT_DIR: &str = ".agent-context";

pub fn get_project_rules_path(project_path: &Path) -> PathBuf {
    project_path.join(AGENT_CONTEXT_DIR).join(TOOL_RULES_FILE)
}

#[allow(dead_code)]
pub fn has_project_rules(project_path: &Path) -> bool {
    get_project_rules_path(project_path).is_file()
}

pub fn load_project_rules(project_path: &Path) -> Option<ToolRulesConfig> {
    let path = get_project_rules_path(project_path);
    if !path.is_file() {
        return None;
    }
    let content = fs::read_to_string(&path).ok()?;
    toml::from_str::<ToolRulesConfig>(&content).ok()
}

pub fn save_project_rules(project_path: &Path, config: &ToolRulesConfig) -> Result<()> {
    let dir = project_path.join(AGENT_CONTEXT_DIR);
    if !dir.exists() {
        fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create directory: {}", dir.display()))?;
    }
    let path = dir.join(TOOL_RULES_FILE);
    let toml_str = toml::to_string_pretty(config)
        .context("Failed to serialize project tool rules to TOML")?;
    fs::write(&path, toml_str)
        .with_context(|| format!("Failed to write project rules: {}", path.display()))?;
    Ok(())
}

pub fn delete_project_rules(project_path: &Path) -> Result<bool> {
    let path = get_project_rules_path(project_path);
    if path.is_file() {
        fs::remove_file(&path)
            .with_context(|| format!("Failed to delete project rules: {}", path.display()))?;
        Ok(true)
    } else {
        Ok(false)
    }
}
