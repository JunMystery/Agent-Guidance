use anyhow::Result;
use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::setup::run_setup;

const REPO: &str = "JunMystery/Agent-Guidance";
const MAX_DOWNLOAD_BYTES: u64 = 256 * 1024 * 1024;

fn is_agent_guidance_process_name(name: &str) -> bool {
    matches!(
        name.trim().to_ascii_lowercase().as_str(),
        "agent-guidance" | "agent-guidance.exe"
    )
}

fn escape_ps_single_quoted(s: &str) -> String {
    s.replace('\'', "''")
}

fn parse_sha256_file(text: &str) -> Option<String> {
    let token = text.split_whitespace().next()?.trim().to_ascii_lowercase();
    if token.len() == 64 && token.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(token)
    } else {
        None
    }
}

fn download_bytes_bounded(url: &str, timeout_secs: u64, max_bytes: u64) -> Result<Vec<u8>> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(timeout_secs))
        .build();
    let resp = agent.get(url).call()?;
    let mut reader = resp.into_reader().take(max_bytes + 1);
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf)?;
    if buf.len() as u64 > max_bytes {
        anyhow::bail!("Downloaded payload from {} exceeds size limit", url);
    }
    Ok(buf)
}

fn download_to_file(url: &str, dest: &Path) -> Result<()> {
    let bytes = download_bytes_bounded(url, 120, MAX_DOWNLOAD_BYTES)?;
    if bytes.is_empty() {
        anyhow::bail!("Downloaded file from {} is empty", url);
    }
    fs::write(dest, bytes)?;
    Ok(())
}

fn get_release_asset_name() -> Option<&'static str> {
    if cfg!(target_os = "windows") {
        Some("agent-guidance-windows-x86_64.zip")
    } else if cfg!(target_os = "linux") {
        Some("agent-guidance-linux-x86_64.tar.gz")
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            Some("agent-guidance-macos-aarch64.tar.gz")
        } else {
            Some("agent-guidance-macos-x86_64.tar.gz")
        }
    } else {
        None
    }
}

pub fn get_target_bin_path() -> Result<PathBuf> {
    let current_exe = env::current_exe().unwrap_or_else(|_| PathBuf::from("agent-guidance"));
    let target = if cfg!(windows) {
        dirs::data_local_dir()
            .map(|d| {
                d.join("Programs")
                    .join("agent-guidance")
                    .join("bin")
                    .join("agent-guidance.exe")
            })
            .unwrap_or(current_exe)
    } else {
        dirs::home_dir()
            .map(|h| h.join(".local").join("bin").join("agent-guidance"))
            .unwrap_or(current_exe)
    };
    Ok(target)
}

fn stop_other_agent_guidance_processes() {
    let my_pid = std::process::id();
    let mut sys = sysinfo::System::new_all();
    sys.refresh_all();
    let mut killed_any = false;
    for (pid, proc_) in sys.processes() {
        if pid.as_u32() != my_pid {
            let name = proc_.name().to_string();
            if is_agent_guidance_process_name(&name) {
                let _ = proc_.kill();
                killed_any = true;
            }
        }
    }
    if killed_any {
        println!("  Terminated running background agent-guidance instances.");
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

pub fn run_upgrade() -> Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");
    println!("Current version: v{}", current_version);
    println!("Checking for latest release package from GitHub ({})...", REPO);

    let asset_name = get_release_asset_name()
        .ok_or_else(|| anyhow::anyhow!("Unsupported operating system or architecture for automatic upgrade"))?;

    let staging_root = dirs::home_dir()
        .map(|h| h.join(".agent-guidance").join("staging"))
        .unwrap_or_else(env::temp_dir);
    let tmp_dir = staging_root.join(format!("ag-upgrade-{}", std::process::id()));
    fs::create_dir_all(&tmp_dir)?;

    let download_url = format!("https://github.com/{}/releases/latest/download/{}", REPO, asset_name);
    let sha_url = format!("{}.sha256", download_url);
    let archive_path = tmp_dir.join(asset_name);

    println!("  Downloading {}...", download_url);
    if let Err(e) = download_to_file(&download_url, &archive_path) {
        let _ = fs::remove_dir_all(&tmp_dir);
        anyhow::bail!("Failed to download release archive from {}: {}", download_url, e);
    }

    let sha_bytes = match download_bytes_bounded(&sha_url, 30, 4096) {
        Ok(b) => b,
        Err(e) => {
            let _ = fs::remove_dir_all(&tmp_dir);
            anyhow::bail!("Failed to download SHA-256 checksum from {}: {}", sha_url, e);
        }
    };
    let sha_text = String::from_utf8_lossy(&sha_bytes);
    let expected_sha = match parse_sha256_file(&sha_text) {
        Some(h) => h,
        None => {
            let _ = fs::remove_dir_all(&tmp_dir);
            anyhow::bail!("Invalid SHA-256 checksum file from {}", sha_url);
        }
    };
    let actual_sha = match crate::mcp::fingerprint::compute_file_sha256(&archive_path) {
        Ok(h) => h.to_ascii_lowercase(),
        Err(e) => {
            let _ = fs::remove_dir_all(&tmp_dir);
            anyhow::bail!("Failed to hash downloaded archive: {}", e);
        }
    };
    if actual_sha != expected_sha {
        let _ = fs::remove_dir_all(&tmp_dir);
        anyhow::bail!(
            "SHA-256 checksum mismatch for {}: expected {}, got {}",
            asset_name,
            expected_sha,
            actual_sha
        );
    }
    println!("  SHA-256 checksum verified.");

    println!("  Download complete. Extracting binary...");
    let extracted_bin = if cfg!(windows) {
        let escaped_archive = escape_ps_single_quoted(&archive_path.to_string_lossy().replace('\\', "/"));
        let escaped_dest = escape_ps_single_quoted(&tmp_dir.to_string_lossy().replace('\\', "/"));
        let ps_cmd = format!(
            "Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
            escaped_archive, escaped_dest
        );
        let status = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &ps_cmd])
            .status();
        if !matches!(status, Ok(s) if s.success()) {
            let _ = fs::remove_dir_all(&tmp_dir);
            anyhow::bail!("Failed to extract zip archive");
        }
        tmp_dir.join("agent-guidance.exe")
    } else {
        let status = std::process::Command::new("tar")
            .args(["-xzf", &archive_path.to_string_lossy(), "-C", &tmp_dir.to_string_lossy()])
            .status();
        if !matches!(status, Ok(s) if s.success()) {
            let _ = fs::remove_dir_all(&tmp_dir);
            anyhow::bail!("Failed to extract tar archive");
        }
        tmp_dir.join("agent-guidance")
    };

    if !extracted_bin.exists() {
        let _ = fs::remove_dir_all(&tmp_dir);
        anyhow::bail!("Extracted archive did not contain agent-guidance binary");
    }

    stop_other_agent_guidance_processes();

    let target_bin = get_target_bin_path()?;
    if let Some(parent) = target_bin.parent() {
        fs::create_dir_all(parent)?;
    }

    println!("  Installing updated binary to {:?}", target_bin);

    // On Windows, if replacing a locked binary, move the existing binary out of the way first
    if target_bin.exists() {
        let old_backup = target_bin.with_extension("old");
        let _ = fs::remove_file(&old_backup);
        let _ = fs::rename(&target_bin, &old_backup);
    }

    fs::copy(&extracted_bin, &target_bin)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&target_bin)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&target_bin, perms)?;
    }

    // Remove legacy cargo-installed binary to prevent PATH shadowing
    if let Some(home) = dirs::home_dir() {
        let cargo_bin = home.join(".cargo").join("bin").join(if cfg!(windows) {
            "agent-guidance.exe"
        } else {
            "agent-guidance"
        });
        if cargo_bin.exists() && cargo_bin != target_bin {
            let _ = fs::remove_file(&cargo_bin);
        }
    }

    let _ = fs::remove_dir_all(&tmp_dir);
    if let Ok(mut entries) = fs::read_dir(&staging_root) {
        if entries.next().is_none() {
            let _ = fs::remove_dir(&staging_root);
        }
    }

    println!("Successfully installed the latest release binary!");
    println!();
    println!("Configuring MCP server across all IDE clients...");
    run_setup(&target_bin)?;

    println!("Agent Guidance successfully upgraded to the latest version!");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_name_exact_match() {
        assert!(is_agent_guidance_process_name("agent-guidance"));
        assert!(is_agent_guidance_process_name("Agent-Guidance.EXE"));
        assert!(!is_agent_guidance_process_name("agent-guidance-helper"));
        assert!(!is_agent_guidance_process_name("not-agent-guidance.exe"));
    }

    #[test]
    fn test_escape_ps_single_quoted() {
        assert_eq!(escape_ps_single_quoted("C:/Users/O'Brien/file.zip"), "C:/Users/O''Brien/file.zip");
    }

    #[test]
    fn test_parse_sha256_file() {
        let valid = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef  agent-guidance-windows-x86_64.zip\n";
        assert_eq!(
            parse_sha256_file(valid).as_deref(),
            Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
        );
        assert!(parse_sha256_file("short  file.zip").is_none());
        assert!(parse_sha256_file("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz  f").is_none());
    }
}
