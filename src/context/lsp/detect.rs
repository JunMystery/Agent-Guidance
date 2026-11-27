//! Workspace language server auto-detection and configuration discovery.

use serde_json::Value;
use std::path::Path;

/// Configuration for launching an LSP server process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspServerConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
}

/// Detects an appropriate language server for the workspace.
pub fn detect_lsp_server(proj_path: &Path) -> Option<LspServerConfig> {
    // 1. Check user-defined config: .agent-context/lsp.json
    let config_file = proj_path.join(".agent-context").join("lsp.json");
    if let Ok(content) = std::fs::read_to_string(&config_file) {
        if let Ok(val) = serde_json::from_str::<Value>(&content) {
            if let Some(command) = val
                .get("command")
                .and_then(|c| c.as_str())
                .map(str::trim)
                .filter(|c| !c.is_empty())
            {
                let name = val
                    .get("name")
                    .and_then(|n| n.as_str())
                    .map(str::trim)
                    .filter(|n| !n.is_empty())
                    .unwrap_or(command);
                let args: Vec<String> = val
                    .get("args")
                    .and_then(|a| a.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();

                if is_command_available(command) {
                    return Some(LspServerConfig {
                        name: name.to_string(),
                        command: command.to_string(),
                        args,
                    });
                }
            }
        }
    }

    // 2. Project type auto-detection by indicator files
    // Rust
    if proj_path.join("Cargo.toml").exists() && is_command_available("rust-analyzer") {
        return Some(LspServerConfig {
            name: "rust-analyzer".to_string(),
            command: "rust-analyzer".to_string(),
            args: Vec::new(),
        });
    }

    // Go
    if proj_path.join("go.mod").exists() && is_command_available("gopls") {
        return Some(LspServerConfig {
            name: "gopls".to_string(),
            command: "gopls".to_string(),
            args: Vec::new(),
        });
    }

    // Python
    if proj_path.join("pyproject.toml").exists()
        || proj_path.join("requirements.txt").exists()
        || proj_path.join("setup.py").exists()
    {
        for candidate in ["pyright-langserver", "pyright", "pylsp"] {
            if is_command_available(candidate) {
                let args = if candidate == "pyright-langserver" {
                    vec!["--stdio".to_string()]
                } else {
                    Vec::new()
                };
                return Some(LspServerConfig {
                    name: candidate.to_string(),
                    command: candidate.to_string(),
                    args,
                });
            }
        }
    }

    // TypeScript / JavaScript
    if proj_path.join("package.json").exists() || proj_path.join("tsconfig.json").exists() {
        for candidate in ["typescript-language-server", "vtsls"] {
            if is_command_available(candidate) {
                return Some(LspServerConfig {
                    name: candidate.to_string(),
                    command: candidate.to_string(),
                    args: vec!["--stdio".to_string()],
                });
            }
        }
    }

    None
}

/// Checks whether an executable exists in system PATH without spawning processes.
pub fn is_command_available(cmd: &str) -> bool {
    let cmd = cmd.trim();
    if cmd.is_empty() {
        return false;
    }

    let extensions: &[&str] = if cfg!(windows) {
        &["", ".exe", ".cmd", ".bat"]
    } else {
        &[""]
    };

    let cmd_path = Path::new(cmd);
    if cmd_path.is_absolute() || cmd_path.components().count() > 1 {
        if cmd_path.is_file() {
            return true;
        }
        if cfg!(windows) && cmd_path.extension().is_none() {
            for ext in &extensions[1..] {
                if Path::new(&format!("{}{}", cmd, ext)).is_file() {
                    return true;
                }
            }
        }
        return false;
    }

    let path_var = match std::env::var_os("PATH") {
        Some(val) => val,
        None => return false,
    };

    for dir in std::env::split_paths(&path_var) {
        for ext in extensions {
            let candidate = dir.join(format!("{}{}", cmd, ext));
            if candidate.is_file() {
                return true;
            }
        }
    }
    false
}
