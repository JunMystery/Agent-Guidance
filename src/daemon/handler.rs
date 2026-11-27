use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{Mutex, OwnedMutexGuard, Semaphore};
use tokio::time::timeout;
use tracing::{error, info};

use crate::mcp::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::mcp::router::handle_request;
use crate::mcp::state::ServerState;

const MAX_REQUEST_WORKERS: usize = 32;
static REQUEST_WORKERS: std::sync::OnceLock<Arc<Semaphore>> = std::sync::OnceLock::new();

struct CancellationGuard {
    state: OwnedMutexGuard<ServerState>,
}

impl CancellationGuard {
    fn new(mut state: OwnedMutexGuard<ServerState>, cancellation: Arc<AtomicBool>) -> Self {
        state.set_cancellation(cancellation);
        Self { state }
    }
}

impl Drop for CancellationGuard {
    fn drop(&mut self) {
        self.state.clear_cancellation();
    }
}

fn configured_request_timeout_secs() -> u64 {
    std::env::var("AGENT_GUIDANCE_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|&v| v > 0)
        .unwrap_or(60)
}

async fn read_bounded_line<R: AsyncBufReadExt + Unpin>(
    reader: &mut R,
    max_bytes: usize,
) -> std::io::Result<Option<Result<String, ()>>> {
    let mut buf = Vec::new();
    let mut oversized = false;
    loop {
        let avail = reader.fill_buf().await?;
        if avail.is_empty() {
            if buf.is_empty() && !oversized {
                return Ok(None);
            }
            if oversized {
                return Ok(Some(Err(())));
            }
            if buf.ends_with(b"\r") {
                buf.pop();
            }
            let s = String::from_utf8(buf)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            return Ok(Some(Ok(s)));
        }
        if let Some(pos) = avail.iter().position(|&b| b == b'\n') {
            if oversized || buf.len() + pos > max_bytes {
                reader.consume(pos + 1);
                return Ok(Some(Err(())));
            }
            buf.extend_from_slice(&avail[..pos]);
            reader.consume(pos + 1);
            if buf.ends_with(b"\r") {
                buf.pop();
            }
            let s = String::from_utf8(buf)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            return Ok(Some(Ok(s)));
        }
        let len = avail.len();
        if !oversized {
            if buf.len() + len > max_bytes {
                oversized = true;
                buf.clear();
                buf.shrink_to_fit();
            } else {
                buf.extend_from_slice(avail);
            }
        }
        reader.consume(len);
    }
}

pub async fn handle_mcp_lines<R, W>(reader: R, mut writer: W)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
    W: tokio::io::AsyncWrite + Unpin + Send,
{
    let mut reader = BufReader::new(reader);
    let client_name = std::env::var("AGENT_CLIENT_NAME").ok();
    let state = Arc::new(Mutex::new(ServerState::with_client_name(client_name)));

    // Eagerly warm up ML models in background only when running local standalone (Zero-ML Client Invariant)
    if !crate::config::current_config().server.is_remote() {
        crate::ml::embeddings::spawn_background_auto_warmup();
    }

    const MAX_LINE_BYTES: usize = 10 * 1024 * 1024;
    let request_timeout_secs = configured_request_timeout_secs();

    loop {
        let line = match read_bounded_line(&mut reader, MAX_LINE_BYTES).await {
            Ok(Some(Ok(l))) => l,
            Ok(Some(Err(()))) => {
                error!(
                    "Request line exceeded max limit of {} bytes",
                    MAX_LINE_BYTES
                );
                let err_resp = JsonRpcResponse::error(
                    serde_json::Value::Null,
                    -32600,
                    "Invalid Request: payload too large",
                );
                let out = serde_json::to_string(&err_resp).unwrap_or_default() + "\n";
                if let Err(e) = writer.write_all(out.as_bytes()).await {
                    error!("Write error: {}", e);
                    break;
                }
                if let Err(e) = writer.flush().await {
                    error!("Flush error: {}", e);
                    break;
                }
                continue;
            }
            Ok(None) => {
                info!("Client disconnected (EOF on stdio stream). Session terminated cleanly.");
                crate::mcp::mcp_logger::log_mcp(
                    crate::mcp::mcp_logger::LogLevel::Info,
                    "mcp_stdio",
                    "Client disconnected (EOF on stdio stream). Session terminated cleanly.",
                    None,
                    None,
                );
                break;
            }
            Err(e) => {
                error!("MCP read error: {}", e);
                crate::mcp::mcp_logger::log_mcp(
                    crate::mcp::mcp_logger::LogLevel::Error,
                    "mcp_stdio",
                    &format!("MCP stream read error: {}", e),
                    None,
                    None,
                );
                break;
            }
        };

        if line.trim().is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(req) => req,
            Err(e) => {
                error!("Invalid JSON-RPC request: {}", e);
                let err_resp = JsonRpcResponse::error(
                    serde_json::Value::Null,
                    -32700,
                    format!("Parse error: {}", e),
                );
                let out = serde_json::to_string(&err_resp).unwrap_or_default() + "\n";
                if let Err(e) = writer.write_all(out.as_bytes()).await {
                    error!("Write error: {}", e);
                    break;
                }
                if let Err(e) = writer.flush().await {
                    error!("Flush error: {}", e);
                    break;
                }
                continue;
            }
        };

        if request.jsonrpc != "2.0" {
            error!("Unsupported JSON-RPC version: {}", request.jsonrpc);
            if let Some(id) = request.id {
                let err_resp = JsonRpcResponse::error(
                    id,
                    -32600,
                    format!(
                        "Invalid Request: jsonrpc must be '2.0', got '{}'",
                        request.jsonrpc
                    ),
                );
                let out = serde_json::to_string(&err_resp).unwrap_or_default() + "\n";
                if let Err(e) = writer.write_all(out.as_bytes()).await {
                    error!("Write error: {}", e);
                    break;
                }
                if let Err(e) = writer.flush().await {
                    error!("Flush error: {}", e);
                    break;
                }
            }
            continue;
        }

        let req_id = request.id.clone();
        let request_method = request.method.clone();
        let timeout_method = request_method.clone();
        let request_params = request.params;
        let cancellation = Arc::new(AtomicBool::new(false));
        let worker_cancellation = cancellation.clone();

        // Keep synchronous parsing, filesystem, indexing, and ML work off Tokio's I/O workers.
        let worker_pool = REQUEST_WORKERS
            .get_or_init(|| Arc::new(Semaphore::new(MAX_REQUEST_WORKERS)))
            .clone();
        let permit = match timeout(
            Duration::from_secs(request_timeout_secs),
            worker_pool.acquire_owned(),
        )
        .await
        {
            Ok(Ok(permit)) => permit,
            Ok(Err(_)) => {
                error!("Request worker pool closed");
                continue;
            }
            Err(_) => {
                error!(
                    "Request worker pool wait exceeded {}s",
                    request_timeout_secs
                );
                if let Some(id) = req_id.clone() {
                    let resp =
                        JsonRpcResponse::error(id, -32000, "Request worker pool is saturated");
                    let out = serde_json::to_string(&resp).unwrap_or_default() + "\n";
                    let _ = writer.write_all(out.as_bytes()).await;
                    let _ = writer.flush().await;
                }
                continue;
            }
        };

        // Acquire the async state lock with bounded timeout so a stuck worker never causes permanent lock starvation.
        let state_guard = match timeout(
            Duration::from_secs(request_timeout_secs),
            state.clone().lock_owned(),
        )
        .await
        {
            Ok(guard) => guard,
            Err(_) => {
                drop(permit);
                error!(
                    "Request state lock wait exceeded {}s: {}",
                    request_timeout_secs, timeout_method
                );
                if let Some(id) = req_id {
                    let resp = JsonRpcResponse::error(
                        id,
                        -32000,
                        format!(
                            "Server state busy: previous request exceeded {}s timeout",
                            request_timeout_secs
                        ),
                    );
                    let out = serde_json::to_string(&resp).unwrap_or_default() + "\n";
                    let _ = writer.write_all(out.as_bytes()).await;
                    let _ = writer.flush().await;
                }
                continue;
            }
        };

        let result = match timeout(
            Duration::from_secs(request_timeout_secs),
            tokio::task::spawn_blocking(move || {
                let _permit = permit;
                let started = Instant::now();
                let mut guard = CancellationGuard::new(state_guard, worker_cancellation);
                let res = if guard.state.is_cancelled() {
                    Err((-32000, "Request cancelled".to_string()))
                } else {
                    handle_request(&request_method, request_params, &mut guard.state)
                };
                info!(
                    method = %request_method,
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "MCP request completed"
                );
                res
            }),
        )
        .await
        {
            Ok(Ok(res)) => res,
            Ok(Err(e)) => Err((-32000, format!("Request worker failed: {}", e))),
            Err(_) => {
                cancellation.store(true, Ordering::Relaxed);
                error!(
                    "Request timed out after {}s: {}",
                    request_timeout_secs, timeout_method
                );
                if let Some(id) = req_id {
                    let resp = JsonRpcResponse::error(
                        id,
                        -32000,
                        format!("Request timed out after {}s", request_timeout_secs),
                    );
                    let out = serde_json::to_string(&resp).unwrap_or_default() + "\n";
                    let _ = writer.write_all(out.as_bytes()).await;
                    let _ = writer.flush().await;
                }
                continue;
            }
        };

        match result {
            Ok(resp_value) => {
                if let Some(id) = req_id {
                    let resp = JsonRpcResponse::success(id, resp_value);
                    let out = serde_json::to_string(&resp).unwrap_or_default() + "\n";
                    if let Err(e) = writer.write_all(out.as_bytes()).await {
                        error!("Write error: {}", e);
                        break;
                    }
                    if let Err(e) = writer.flush().await {
                        error!("Flush error: {}", e);
                        break;
                    }
                }
            }
            Err((code, msg)) => {
                if let Some(id) = req_id {
                    let resp = JsonRpcResponse::error(id, code, msg);
                    let out = serde_json::to_string(&resp).unwrap_or_default() + "\n";
                    if let Err(e) = writer.write_all(out.as_bytes()).await {
                        error!("Write error: {}", e);
                        break;
                    }
                    if let Err(e) = writer.flush().await {
                        error!("Flush error: {}", e);
                        break;
                    }
                }
            }
        }
    }

    if let Ok(guard) = state.try_lock()
        && let Some(ref proj_str) = guard.project_path
    {
        let proj = Path::new(proj_str);
        if proj.is_dir()
            && (guard.tool_calls > 0
                || !guard.modified_files.is_empty()
                || !guard.recent_tasks.is_empty()
                || guard.plan_approved)
        {
            let _ = guard.save_to_dir(proj);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    #[tokio::test]
    async fn test_handler_ignores_notification_errors_and_updates_state_for_reads() {
        let (client_tx, server_rx) = tokio::io::duplex(4096);
        let (server_tx, client_rx) = tokio::io::duplex(4096);

        let handle = tokio::spawn(async move {
            handle_mcp_lines(server_rx, server_tx).await;
        });

        let mut writer = client_tx;
        let mut reader = BufReader::new(client_rx).lines();

        // 1. Notification with wrong jsonrpc version (id: None) -> MUST NOT emit error response
        writer
            .write_all(b"{\"jsonrpc\":\"1.0\",\"method\":\"ping\"}\n")
            .await
            .unwrap();
        // 2. Notification with unknown method (id: None) -> MUST NOT emit error response
        writer
            .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/unknown\"}\n")
            .await
            .unwrap();
        // 3. Real request with id: 1 -> first response MUST be id: 1
        writer
            .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n")
            .await
            .unwrap();
        // 4. Read-only resource read + gate check still runs cleanly on live ServerState
        writer
            .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"resources/read\",\"params\":{\"uri\":\"agent-guidance://system/edit-allowed\"}}\n")
            .await
            .unwrap();
        writer.flush().await.unwrap();
        drop(writer);

        let line1 = reader.next_line().await.unwrap().unwrap();
        let val1: serde_json::Value = serde_json::from_str(&line1).unwrap();
        assert_eq!(val1["id"], 1);
        assert!(val1.get("result").is_some());

        let line2 = reader.next_line().await.unwrap().unwrap();
        let val2: serde_json::Value = serde_json::from_str(&line2).unwrap();
        assert_eq!(val2["id"], 2);
        assert!(val2.get("result").is_some());

        assert!(reader.next_line().await.unwrap().is_none());
        handle.await.unwrap();
    }
}
