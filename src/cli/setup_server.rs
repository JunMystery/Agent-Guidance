use std::io::{self, Write};
use anyhow::Result;

pub fn run_setup_server_wizard() -> Result<bool> {
    println!("============================================================");
    println!("  Agent Guidance — Server Worker Interactive Setup Wizard   ");
    println!("============================================================");

    let bind = prompt_default("Listen Address", "0.0.0.0");
    if !is_safe_arg_token(&bind) || bind.is_empty() {
        anyhow::bail!("Invalid listen address: contains unsafe characters or is empty");
    }
    let worker_port_str = prompt_default("ML Worker API Port", "11998");
    let worker_port: u16 = worker_port_str.parse().unwrap_or(11998);

    let dashboard_port_str = prompt_default("Admin Dashboard Port", "11997");
    let dashboard_port: u16 = dashboard_port_str.parse().unwrap_or(11997);

    let api_key = prompt_default("Bearer API Key (required for non-loopback bind)", "");
    if !is_safe_arg_token(&api_key) {
        anyhow::bail!("Invalid API key: contains whitespace or unsafe shell characters");
    }
    if !crate::ml::worker::is_loopback_bind(&bind) && api_key.is_empty() {
        anyhow::bail!("Refusing to generate config for non-loopback bind '{}' without an API key", bind);
    }

    println!();
    println!("Generating service configuration scripts for your OS...");

    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "agent-guidance".to_string());

    if cfg!(target_os = "linux") {
        let systemd_content = generate_linux_systemd(&exe, &bind, worker_port, dashboard_port, &api_key);
        if let Some(home) = dirs::home_dir() {
            let user_unit = home.join(".config").join("systemd").join("user").join("agent-guidance.service");
            if let Some(parent) = user_unit.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if std::fs::write(&user_unit, &systemd_content).is_ok() {
                set_private_file_perms(&user_unit);
                println!("[OK] Generated systemd user unit at: {:?}", user_unit);
                println!("  To activate: systemctl --user daemon-reload && systemctl --user enable --now agent-guidance");
            }
        }
    } else if cfg!(target_os = "macos") {
        let launchd_content = generate_macos_launchd(&exe, &bind, worker_port, dashboard_port, &api_key);
        if let Some(home) = dirs::home_dir() {
            let plist = home.join("Library").join("LaunchAgents").join("com.junmystery.agent-guidance.plist");
            if let Some(parent) = plist.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if std::fs::write(&plist, &launchd_content).is_ok() {
                set_private_file_perms(&plist);
                println!("[OK] Generated launchd plist at: {:?}", plist);
                println!("  To activate: launchctl load {:?}", plist);
            }
        }
    } else if cfg!(target_os = "windows") {
        let sch_cmd = generate_windows_task_cmd(&exe, &bind, worker_port, dashboard_port, &api_key);
        if !api_key.is_empty() {
            println!("  [IMPORTANT] Persist AGENT_GUIDANCE_API_KEY in user env before creating the scheduled task:");
            println!("    setx AGENT_GUIDANCE_API_KEY \"<YOUR_API_KEY>\"");
        }
        println!("[OK] Windows Task Scheduler command:");
        println!("  {}", sch_cmd);
    }

    println!();
    println!("[OK] Server configuration ready.");
    println!("  Run directly: {} --server --bind {} --worker-port {} --port {}", exe, bind, worker_port, dashboard_port);
    println!("============================================================");
    Ok(true)
}

fn is_safe_arg_token(s: &str) -> bool {
    !s.chars().any(|c| {
        c.is_control()
            || c.is_whitespace()
            || matches!(c, '"' | '\'' | '\\' | '$' | '`' | '&' | '|' | ';' | '<' | '>')
    })
}

fn set_private_file_perms(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    let _ = path;
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn escape_systemd_env_val(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('%', "%%")
}

fn prompt_default(prompt: &str, default: &str) -> String {
    print!("? {} [{}]: ", prompt, default);
    let _ = io::stdout().flush();
    let mut input = String::new();
    let _ = io::stdin().read_line(&mut input);
    let trimmed = input.trim();
    if trimmed.is_empty() {
        default.to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn generate_linux_systemd(bin: &str, bind: &str, w_port: u16, d_port: u16, key: &str) -> String {
    let env_line = if key.is_empty() {
        String::new()
    } else {
        format!("Environment=\"AGENT_GUIDANCE_API_KEY={}\"\n", escape_systemd_env_val(key))
    };
    format!(
        "[Unit]\nDescription=Agent Guidance Remote ML Worker\nAfter=network.target\n\n[Service]\nType=simple\n{}ExecStart=\"{}\" --server --bind {} --worker-port {} --port {}\nRestart=always\nRestartSec=5\n\n[Install]\nWantedBy=default.target\n",
        env_line, bin, bind, w_port, d_port
    )
}

pub fn generate_macos_launchd(bin: &str, bind: &str, w_port: u16, d_port: u16, key: &str) -> String {
    let env_block = if key.is_empty() {
        String::new()
    } else {
        format!(
            "<key>EnvironmentVariables</key>\n<dict><key>AGENT_GUIDANCE_API_KEY</key><string>{}</string></dict>\n",
            escape_xml(key)
        )
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n<key>Label</key><string>com.junmystery.agent-guidance</string>\n{}<key>ProgramArguments</key>\n<array>\n<string>{}</string>\n<string>--server</string>\n<string>--bind</string>\n<string>{}</string>\n<string>--worker-port</string>\n<string>{}</string>\n<string>--port</string>\n<string>{}</string>\n</array>\n<key>RunAtLoad</key><true/>\n<key>KeepAlive</key><true/>\n</dict>\n</plist>\n",
        env_block, escape_xml(bin), escape_xml(bind), w_port, d_port
    )
}

pub fn generate_windows_task_cmd(bin: &str, bind: &str, w_port: u16, d_port: u16, _key: &str) -> String {
    format!(
        "schtasks /create /tn \"AgentGuidanceServer\" /tr \"\\\"{}\\\" --server --bind {} --worker-port {} --port {}\" /sc onlogon /rl highest /f",
        bin, bind, w_port, d_port
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_configs_omit_cli_api_key_and_escape_xml() {
        let systemd = generate_linux_systemd("/usr/local/bin/ag", "0.0.0.0", 11998, 11997, "sec_123");
        assert!(systemd.contains("AGENT_GUIDANCE_API_KEY=sec_123"));
        assert!(!systemd.contains("--api-key"));

        let plist = generate_macos_launchd("/Applications/Agent&Co/ag", "0.0.0.0", 11998, 11997, "k<1>&2");
        assert!(plist.contains("Agent&amp;Co"));
        assert!(plist.contains("k&lt;1&gt;&amp;2"));
        assert!(!plist.contains("--api-key"));

        let task = generate_windows_task_cmd("C:\\ag.exe", "0.0.0.0", 11998, 11997, "secret");
        assert!(!task.contains("secret"));
        assert!(!task.contains("--api-key"));
    }
}
