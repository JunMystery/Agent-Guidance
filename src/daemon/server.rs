use tracing::{error, info};
use std::time::Duration;

use super::{
    acquire_daemon_lock, client_connected, client_disconnected,
};
use super::handler::handle_mcp_lines;

use super::lifecycle::{DEFAULT_STARTUP_TIMEOUT_SECS, monitor_client_lifecycle};

#[cfg(unix)]
pub async fn daemon_main(port: u16, project_path: Option<String>) {
    use std::fs;
    use tokio::net::UnixListener;
    use super::socket_path;

    let _lock = match acquire_daemon_lock() {
        Some(l) => l,
        None => {
            info!("Daemon lock held by another process. Exiting.");
            return;
        }
    };


    let path = socket_path();
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            error!("Failed to create socket directory: {}", e);
            return;
        }
    }
    let _ = fs::remove_file(&path);

    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            error!("Failed to bind socket: {}", e);
            return;
        }
    };
    info!("Daemon listening on Unix socket: {:?}", path);

    // Background ML model warmup
    tokio::spawn(async {
        let warmup = tokio::task::spawn_blocking(|| {
            let _ = crate::ml::embeddings::eager_vram_warmup();
        });
        if let Err(e) = warmup.await {
            error!("Model warmup failed: {:?}", e);
        } else {
            info!("Background VRAM model & skill matrix residency warmup completed.");
        }
    });

    crate::daemon::tray::spawn_system_tray(port);
    crate::dashboard::spawn_dashboard_background(port, project_path);

    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
    let accept_handle = tokio::spawn(async move {
        let mut backoff_ms = 50u64;
        loop {
            tokio::select! {
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        break;
                    }
                }
                res = listener.accept() => {
                    match res {
                        Ok((stream, _)) => {
                            backoff_ms = 50;
                            client_connected();
                            tokio::spawn(async move {
                                let (reader, writer) = stream.into_split();
                                handle_mcp_lines(reader, writer).await;
                                client_disconnected();
                                checkpoint_usage_db();
                            });
                        }
                        Err(e) => {
                            error!("Accept error (retrying in {}ms): {}", backoff_ms, e);
                            tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                            backoff_ms = (backoff_ms * 2).min(500);
                            continue;
                        }
                    }
                }
            }
        }
    });

    let startup_timeout_secs: u64 = std::env::var("AGENT_GUIDANCE_STARTUP_TIMEOUT")
        .or_else(|_| std::env::var("AGENT_GUIDANCE_IDLE_TIMEOUT"))
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_STARTUP_TIMEOUT_SECS);

    monitor_client_lifecycle(startup_timeout_secs).await;
    let _ = shutdown_tx.send(true);
    let _ = tokio::time::timeout(Duration::from_secs(2), accept_handle).await;
    checkpoint_usage_db();
    drop(_lock);
    let _ = fs::remove_file(&path);
    crate::mcp::mcp_logger::log_mcp_direct(
        crate::mcp::mcp_logger::LogLevel::Info,
        "daemon_lifecycle",
        "Daemon idle cooldown reached. Shutting down daemon cleanly.",
        None,
        None,
    );
}

#[cfg(windows)]
pub async fn daemon_main(port: u16, project_path: Option<String>) {
    use tokio::net::windows::named_pipe::ServerOptions;
    use super::WINDOWS_PIPE_NAME;

    let _lock = match acquire_daemon_lock() {
        Some(l) => l,
        None => {
            info!("Daemon lock held by another process. Exiting.");
            return;
        }
    };

    crate::daemon::tray::spawn_system_tray(port);

    let pipe_name = WINDOWS_PIPE_NAME;
    info!("Daemon listening on Windows Named Pipe: {}", pipe_name);

    // Background ML model warmup
    tokio::spawn(async {
        let warmup = tokio::task::spawn_blocking(|| {
            let _ = crate::ml::embeddings::eager_vram_warmup();
        });
        if let Err(e) = warmup.await {
            error!("Model warmup failed: {:?}", e);
        } else {
            info!("Background VRAM model & skill matrix residency warmup completed.");
        }
    });

    crate::dashboard::spawn_dashboard_background(port, project_path);

    let mut current_server = match ServerOptions::new().first_pipe_instance(true).create(pipe_name) {
        Ok(s) => Some(s),
        Err(e) => {
            error!("Failed to create initial named pipe instance: {}", e);
            return;
        }
    };

    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
    let accept_handle = tokio::spawn(async move {
        let mut consecutive_errors = 0u32;
        loop {
            let server = match current_server.take() {
                Some(s) => s,
                None => match ServerOptions::new().create(pipe_name) {
                    Ok(s) => {
                        consecutive_errors = 0;
                        s
                    }
                    Err(e) => {
                        consecutive_errors += 1;
                        error!("Failed to create named pipe instance: {}", e);
                        if consecutive_errors >= 10 {
                            error!("Too many consecutive named pipe creation failures; stopping listener.");
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                },
            };

            let connected = tokio::select! {
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        break;
                    }
                    continue;
                }
                res = server.connect() => res,
            };

            // Pre-create next pipe instance immediately so incoming clients see zero gap.
            if current_server.is_none()
                && let Ok(next_s) = ServerOptions::new().create(pipe_name)
            {
                current_server = Some(next_s);
                consecutive_errors = 0;
            }

            match connected {
                Ok(()) => {
                    client_connected();
                    tokio::spawn(async move {
                        let (reader, writer) = tokio::io::split(server);
                        handle_mcp_lines(reader, writer).await;
                        client_disconnected();
                        checkpoint_usage_db();
                    });
                }
                Err(e) => {
                    error!("Named pipe connect error: {}", e);
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }
        }
    });

    let startup_timeout_secs: u64 = std::env::var("AGENT_GUIDANCE_STARTUP_TIMEOUT")
        .or_else(|_| std::env::var("AGENT_GUIDANCE_IDLE_TIMEOUT"))
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_STARTUP_TIMEOUT_SECS);

    monitor_client_lifecycle(startup_timeout_secs).await;
    let _ = shutdown_tx.send(true);
    let _ = tokio::time::timeout(Duration::from_secs(2), accept_handle).await;
    checkpoint_usage_db();
    drop(_lock);
    crate::mcp::mcp_logger::log_mcp_direct(
        crate::mcp::mcp_logger::LogLevel::Info,
        "daemon_lifecycle",
        "Daemon idle cooldown reached. Shutting down daemon cleanly.",
        None,
        None,
    );
}

fn checkpoint_usage_db() {
    let db_path = crate::mcp::db::get_db_path();
    if db_path.exists() {
        if let Ok(conn) = rusqlite::Connection::open_with_flags(
            &db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ) {
            let _ = conn.busy_timeout(Duration::from_millis(2000));
            let _ = conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE);");
        }
    }
}
