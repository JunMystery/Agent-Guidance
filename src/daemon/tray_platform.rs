use std::path::Path;
use std::process::Command;

/// Open a URL in default system browser
pub fn open_browser(url: &str) {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        let op: Vec<u16> = "open\0".encode_utf16().collect();
        let url_wide: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            ShellExecuteW(
                0,
                op.as_ptr(),
                url_wide.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            );
        }
    }

    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open").arg(url).spawn();
    }

    #[cfg(target_os = "linux")]
    {
        let _ = Command::new("xdg-open").arg(url).spawn();
    }
}

/// Open a directory in native file manager
pub fn open_folder(path: &Path) {
    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("explorer").arg(path).spawn();
    }

    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open").arg(path).spawn();
    }

    #[cfg(target_os = "linux")]
    {
        let _ = Command::new("xdg-open").arg(path).spawn();
    }
}

/// Display native 'About' information dialog
pub fn show_about_dialog(parent_hwnd: isize, version: &str) {
    let title = "About Agent Guidance";
    let message = format!(
        "Agent Guidance MCP Server v{}\n\n\
        The Architectural Conductor for AI Coding Agents.\n\n\
        Inference Engine: Rust Candle / ONNX\n\
        License: MIT\n\
        GitHub: https://github.com/JunMystery/Agent-Guidance",
        version
    );

    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Foundation::HWND;
        use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};

        let title_w: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        let msg_w: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            MessageBoxW(
                parent_hwnd as HWND,
                msg_w.as_ptr(),
                title_w.as_ptr(),
                MB_OK | MB_ICONINFORMATION,
            );
        }
    }

    #[cfg(target_os = "macos")]
    {
        let _ = parent_hwnd;
        let script = format!(
            "display dialog \"{}\" with title \"{}\" buttons {{\"OK\"}} default button \"OK\" with icon note",
            message.replace('"', "\\\""),
            title
        );
        let _ = Command::new("osascript").arg("-e").arg(script).spawn();
    }

    #[cfg(target_os = "linux")]
    {
        let _ = parent_hwnd;
        let zenity = Command::new("zenity")
            .arg("--info")
            .arg(format!("--title={}", title))
            .arg(format!("--text={}", message))
            .spawn();

        if zenity.is_err() {
            let _ = Command::new("notify-send")
                .arg(title)
                .arg(&message)
                .spawn();
        }
    }
}
