pub mod handlers;
pub mod router;
pub mod state;
pub mod types;

#[cfg(test)]
mod tests;

use std::sync::Arc;
use anyhow::{Context, Result};
use tiny_http::Server;
use tracing::info;

pub use state::WorkerState;

pub const DEFAULT_WORKER_PORT: u16 = 11998;
pub const DEFAULT_BIND_ADDR: &str = "127.0.0.1";
const WORKER_THREADS: usize = 4;

pub fn is_loopback_bind(bind_addr: &str) -> bool {
    let norm = bind_addr
        .trim()
        .trim_matches(|c| c == '[' || c == ']')
        .to_ascii_lowercase();
    matches!(norm.as_str(), "127.0.0.1" | "::1" | "localhost")
}

pub fn run_worker_server(bind_addr: &str, port: u16, api_key: String) -> Result<()> {
    if !is_loopback_bind(bind_addr) && api_key.trim().is_empty() {
        anyhow::bail!(
            "Refusing to bind ML Worker to non-loopback address '{}' without --api-key or AGENT_GUIDANCE_API_KEY",
            bind_addr
        );
    }
    let addr = format!("{}:{}", bind_addr, port);
    let server = Arc::new(
        Server::http(&addr).map_err(|e| anyhow::anyhow!("Failed to bind ML Worker on {}: {}", addr, e))?,
    );
    info!("[ML Worker Daemon] Listening on http://{}", addr);

    // Load or compile initial state
    let state = WorkerState::load_from_disk().unwrap_or_else(|_| {
        info!("Initial binaries missing on disk; running one-time compilation...");
        let _ = crate::ml::embeddings::compiler::compile_staging_to_binary(false);
        WorkerState::load_from_disk().unwrap_or_else(|_| WorkerState::new_empty())
    });

    let mut handles = Vec::new();
    for thread_id in 0..WORKER_THREADS {
        let s = Arc::clone(&server);
        let st = state.clone();
        let key = api_key.clone();
        let handle = std::thread::Builder::new()
            .name(format!("ml-worker-thread-{}", thread_id))
            .spawn(move || {
                for req in s.incoming_requests() {
                    router::route_request(req, &st, &key);
                }
            })
            .context("Failed to spawn ML worker thread")?;
        handles.push(handle);
    }

    for h in handles {
        let _ = h.join();
    }
    Ok(())
}
