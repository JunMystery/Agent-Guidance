use std::path::{Path, PathBuf};

pub struct McpTarget {
    pub name: &'static str,
    pub path: PathBuf,
    pub force: bool,
    pub key: &'static str,
}

pub struct ExtensionTarget {
    pub name: &'static str,
    pub path: PathBuf,
}

pub fn get_client_targets(home: &Path) -> (Vec<McpTarget>, Vec<ExtensionTarget>) {
    let app_paths: Option<(PathBuf, PathBuf, PathBuf, PathBuf, PathBuf)> = if cfg!(target_os = "windows") {
        let appdata = std::env::var("APPDATA").unwrap_or_default();
        if !appdata.trim().is_empty() {
            let app_path = PathBuf::from(appdata);
            Some((
                app_path.join("Claude").join("claude_desktop_config.json"),
                app_path.join("Code").join("User"),
                app_path.join("Code - Insiders").join("User"),
                app_path.join("Cursor").join("User"),
                app_path
                    .join("Devin")
                    .join("Cascade")
                    .join("mcp_config.json"),
            ))
        } else {
            None
        }
    } else if cfg!(target_os = "macos") {
        let app_support = home.join("Library").join("Application Support");
        Some((
            app_support.join("Claude").join("claude_desktop_config.json"),
            app_support.join("Code").join("User"),
            app_support.join("Code - Insiders").join("User"),
            app_support.join("Cursor").join("User"),
            app_support
                .join("Devin")
                .join("Cascade")
                .join("mcp_config.json"),
        ))
    } else {
        let cfg_dir = home.join(".config");
        Some((
            cfg_dir.join("Claude").join("claude_desktop_config.json"),
            cfg_dir.join("Code").join("User"),
            cfg_dir.join("Code - Insiders").join("User"),
            cfg_dir.join("Cursor").join("User"),
            cfg_dir
                .join("Devin")
                .join("Cascade")
                .join("mcp_config.json"),
        ))
    };

    let mut targets = vec![
        McpTarget {
            name: "Antigravity / Gemini Global MCP config",
            path: home.join(".gemini").join("config").join("mcp_config.json"),
            force: true,
            key: "mcpServers",
        },
        McpTarget {
            name: "Antigravity Legacy MCP config",
            path: home.join(".gemini").join("antigravity").join("mcp_config.json"),
            force: true,
            key: "mcpServers",
        },
        McpTarget {
            name: "Cursor Native",
            path: home.join(".cursor").join("mcp.json"),
            force: true,
            key: "mcpServers",
        },
        McpTarget {
            name: "GitHub Copilot Agent Host",
            path: home.join(".copilot").join("mcp-config.json"),
            force: false,
            key: "mcpServers",
        },
        McpTarget {
            name: "Continue.dev",
            path: home.join(".continue").join("mcpServers").join("config.json"),
            force: true,
            key: "mcpServers",
        },
        McpTarget {
            name: "Claude Code",
            path: home.join(".claude.json"),
            force: true,
            key: "mcpServers",
        },
        McpTarget {
            name: "Claude Code Legacy",
            path: home.join(".claude").join("mcp.json"),
            force: false,
            key: "mcpServers",
        },
        McpTarget {
            name: "Windsurf",
            path: home.join(".codeium").join("windsurf").join("mcp_config.json"),
            force: true,
            key: "mcpServers",
        },
    ];

    let mut extensions = Vec::new();
    if let Some((claude_path, code_path, code_insiders_path, cursor_path, devin_path)) = app_paths {
        targets.push(McpTarget {
            name: "Claude Desktop",
            path: claude_path,
            force: true,
            key: "mcpServers",
        });
        targets.push(McpTarget {
            name: "Cursor User Profile",
            path: cursor_path.join("mcp.json"),
            force: false,
            key: "mcpServers",
        });
        targets.push(McpTarget {
            name: "VS Code Native",
            path: code_path.join("mcp.json"),
            force: true,
            key: "servers",
        });
        targets.push(McpTarget {
            name: "VS Code Insiders",
            path: code_insiders_path.join("mcp.json"),
            force: false,
            key: "servers",
        });
        targets.push(McpTarget {
            name: "Devin/Cascade",
            path: devin_path,
            force: true,
            key: "mcpServers",
        });

        extensions.push(ExtensionTarget {
            name: "VS Code Cline",
            path: code_path
                .join("globalStorage")
                .join("saoudrizwan.claude-dev")
                .join("settings")
                .join("cline_mcp_settings.json"),
        });
        extensions.push(ExtensionTarget {
            name: "VS Code Roo-Code",
            path: code_path
                .join("globalStorage")
                .join("roovet.roo-cline")
                .join("settings")
                .join("cline_mcp_settings.json"),
        });
        extensions.push(ExtensionTarget {
            name: "Cursor Cline",
            path: cursor_path
                .join("globalStorage")
                .join("saoudrizwan.claude-dev")
                .join("settings")
                .join("cline_mcp_settings.json"),
        });
        extensions.push(ExtensionTarget {
            name: "Cursor Roo-Code",
            path: cursor_path
                .join("globalStorage")
                .join("roovet.roo-cline")
                .join("settings")
                .join("cline_mcp_settings.json"),
        });
    }

    (targets, extensions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_client_targets_produces_absolute_paths_for_absolute_home() {
        let home = std::env::temp_dir().join("ag_targets_test_home");
        let (targets, extensions) = get_client_targets(&home);
        assert!(targets.iter().any(|t| t.name == "Claude Code" && t.path.ends_with(".claude.json")));
        for t in &targets {
            assert!(t.path.is_absolute(), "Target {} path should be absolute: {:?}", t.name, t.path);
        }
        for e in &extensions {
            assert!(e.path.is_absolute(), "Extension {} path should be absolute: {:?}", e.name, e.path);
        }
    }
}
