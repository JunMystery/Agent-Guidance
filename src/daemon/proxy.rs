use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tracing::{error, info, warn};

use crate::mcp::protocol::JsonRpcResponse;
use super::ensure_daemon_running;

#[cfg(unix)]
use tokio::net::UnixStream;
#[cfg(unix)]
use super::socket_path;

#[cfg(windows)]
use tokio::net::windows::named_pipe::ClientOptions;
#[cfg(windows)]
use super::WINDOWS_PIPE_NAME;

const REINIT_SENTINEL_ID: &str = "__ag_proxy_reinit__";
const MAX_RECONNECT_CYCLES: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyOutcome {
    CleanEof,
    InitialConnectFailed,
    DisconnectedMidSession,
}

pub async fn try_proxy_mode() -> bool {
    run_proxy_session().await == ProxyOutcome::CleanEof
}

pub async fn run_proxy_session() -> ProxyOutcome {
    let initial = match connect_daemon_stream(30).await {
        Some(pair) => pair,
        None => return ProxyOutcome::InitialConnectFailed,
    };

    proxy_session_loop(
        tokio::io::stdin(),
        tokio::io::stdout(),
        Some(initial),
        Duration::from_secs(10),
        || Box::pin(connect_daemon_stream(20)),
    )
    .await
}

async fn spawn_daemon_nonblocking() {
    let _ = tokio::task::spawn_blocking(ensure_daemon_running).await;
}

#[cfg(unix)]
async fn connect_daemon_stream(
    max_attempts: usize,
) -> Option<(Box<dyn AsyncRead + Unpin + Send>, Box<dyn AsyncWrite + Unpin + Send>)> {
    let path = socket_path();
    let mut spawned = false;
    for attempt in 0..max_attempts {
        if path.exists() {
            match UnixStream::connect(&path).await {
                Ok(stream) => {
                    info!("Connected to background daemon via Unix socket: {:?}", path);
                    let (rx, tx) = stream.into_split();
                    return Some((Box::new(rx), Box::new(tx)));
                }
                Err(e) => {
                    if attempt == 0 && !spawned {
                        spawn_daemon_nonblocking().await;
                        spawned = true;
                    }
                    if attempt % 5 == 0 {
                        info!("Socket connect pending ({}), retrying...", e);
                    }
                }
            }
        } else if !spawned {
            spawn_daemon_nonblocking().await;
            spawned = true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    info!("Could not connect to background daemon socket.");
    None
}

#[cfg(windows)]
async fn connect_daemon_stream(
    max_attempts: usize,
) -> Option<(Box<dyn AsyncRead + Unpin + Send>, Box<dyn AsyncWrite + Unpin + Send>)> {
    let pipe_name = WINDOWS_PIPE_NAME;
    let mut spawned = false;
    for attempt in 0..max_attempts {
        match ClientOptions::new().open(pipe_name) {
            Ok(client) => {
                info!("Connected to background daemon via Named Pipe: {}", pipe_name);
                let (rx, tx) = tokio::io::split(client);
                return Some((Box::new(rx), Box::new(tx)));
            }
            Err(e) => {
                if !spawned {
                    spawn_daemon_nonblocking().await;
                    spawned = true;
                }
                if attempt % 5 == 0 {
                    info!("Named pipe connect pending ({}), retrying...", e);
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
    info!("Could not connect to background daemon Windows Named Pipe.");
    None
}

fn canonical_id_key(id: &Value) -> String {
    serde_json::to_string(id).unwrap_or_default()
}

async fn emit_pending_errors<W: AsyncWrite + Unpin>(
    stdout: &mut W,
    pending: &mut Vec<(String, Value)>,
    reason: &str,
) {
    for (_, id_val) in pending.drain(..) {
        let resp = JsonRpcResponse::error(id_val, -32000, reason);
        let out = serde_json::to_string(&resp).unwrap_or_default() + "\n";
        if stdout.write_all(out.as_bytes()).await.is_err() {
            break;
        }
    }
    let _ = stdout.flush().await;
}

type ConnectFuture<DR, DW> = Pin<Box<dyn Future<Output = Option<(DR, DW)>> + Send>>;

async fn proxy_session_loop<R, W, DR, DW, F>(
    stdin: R,
    mut stdout: W,
    mut current_conn: Option<(DR, DW)>,
    drain_timeout: Duration,
    mut reconnect: F,
) -> ProxyOutcome
where
    R: AsyncRead + Unpin + Send,
    W: AsyncWrite + Unpin + Send,
    DR: AsyncRead + Unpin + Send,
    DW: AsyncWrite + Unpin + Send,
    F: FnMut() -> ConnectFuture<DR, DW>,
{
    let mut stdin_lines = BufReader::new(stdin).lines();
    let mut pending_requests: Vec<(String, Value)> = Vec::new();
    let mut init_params: Option<Value> = None;
    let mut stdin_eof = false;
    let mut had_traffic = false;
    let mut reconnect_attempts = 0usize;
    let mut is_reconnect = false;

    loop {
        let (socket_rx, mut socket_tx) = match current_conn.take() {
            Some(pair) => pair,
            None => {
                if stdin_eof {
                    return ProxyOutcome::CleanEof;
                }
                if reconnect_attempts >= MAX_RECONNECT_CYCLES {
                    error!(
                        "Failed to reconnect to background daemon after {} attempts",
                        MAX_RECONNECT_CYCLES
                    );
                    return if had_traffic {
                        ProxyOutcome::DisconnectedMidSession
                    } else {
                        ProxyOutcome::InitialConnectFailed
                    };
                }
                reconnect_attempts += 1;
                warn!(
                    "Attempting daemon reconnect ({}/{})...",
                    reconnect_attempts, MAX_RECONNECT_CYCLES
                );
                match reconnect().await {
                    Some(pair) => {
                        is_reconnect = true;
                        pair
                    }
                    None => continue,
                }
            }
        };

        let mut reinit_pending = false;
        if is_reconnect
            && let Some(ref params) = init_params
        {
            let reinit = json!({
                "jsonrpc": "2.0",
                "id": REINIT_SENTINEL_ID,
                "method": "initialize",
                "params": params
            });
            let line = serde_json::to_string(&reinit).unwrap_or_default() + "\n";
            if socket_tx.write_all(line.as_bytes()).await.is_err()
                || socket_tx.flush().await.is_err()
            {
                continue;
            }
            reinit_pending = true;
        }

        let mut daemon_lines = BufReader::new(socket_rx).lines();
        let disconnected: bool;

        loop {
            let idle_drain_wait = if pending_requests.is_empty() {
                drain_timeout.min(Duration::from_millis(50))
            } else {
                drain_timeout
            };

            tokio::select! {
                line_res = stdin_lines.next_line(), if !stdin_eof => {
                    match line_res {
                        Ok(Some(line)) => {
                            had_traffic = true;
                            if let Ok(val) = serde_json::from_str::<Value>(&line) {
                                if val.get("method").and_then(|m| m.as_str()) == Some("initialize") {
                                    init_params = val.get("params").cloned().or_else(|| Some(json!({})));
                                }
                                if let Some(id_val) = val.get("id").filter(|v| !v.is_null()) {
                                    let key = canonical_id_key(id_val);
                                    if !pending_requests.iter().any(|(k, _)| k == &key) {
                                        pending_requests.push((key, id_val.clone()));
                                    }
                                }
                            }
                            let line_bytes = format!("{}\n", line);
                            if socket_tx.write_all(line_bytes.as_bytes()).await.is_err()
                                || socket_tx.flush().await.is_err()
                            {
                                disconnected = true;
                                break;
                            }
                            reconnect_attempts = 0;
                        }
                        Ok(None) => {
                            stdin_eof = true;
                            let _ = socket_tx.shutdown().await;
                        }
                        Err(_) => {
                            stdin_eof = true;
                            let _ = socket_tx.shutdown().await;
                        }
                    }
                }
                daemon_res = daemon_lines.next_line() => {
                    match daemon_res {
                        Ok(Some(line)) => {
                            if let Ok(val) = serde_json::from_str::<Value>(&line)
                                && let Some(id_val) = val.get("id")
                            {
                                if reinit_pending && id_val.as_str() == Some(REINIT_SENTINEL_ID) {
                                    reinit_pending = false;
                                    continue;
                                }
                                let key = canonical_id_key(id_val);
                                pending_requests.retain(|(k, _)| k != &key);
                            }
                            let out = format!("{}\n", line);
                            if stdout.write_all(out.as_bytes()).await.is_err()
                                || stdout.flush().await.is_err()
                            {
                                return ProxyOutcome::CleanEof;
                            }
                            if stdin_eof && pending_requests.is_empty() {
                                return ProxyOutcome::CleanEof;
                            }
                        }
                        Ok(None) | Err(_) => {
                            disconnected = true;
                            break;
                        }
                    }
                }
                _ = tokio::time::sleep(idle_drain_wait), if stdin_eof => {
                    if !pending_requests.is_empty() {
                        emit_pending_errors(
                            &mut stdout,
                            &mut pending_requests,
                            "Timed out waiting for daemon response after stdin EOF",
                        )
                        .await;
                    }
                    return ProxyOutcome::CleanEof;
                }
            }
        }

        if disconnected {
            if !pending_requests.is_empty() {
                emit_pending_errors(
                    &mut stdout,
                    &mut pending_requests,
                    "Background daemon disconnected mid-session before completing request",
                )
                .await;
                if stdin_eof {
                    return ProxyOutcome::DisconnectedMidSession;
                }
            } else if stdin_eof {
                return ProxyOutcome::CleanEof;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn test_proxy_drains_pending_responses_after_stdin_eof() {
        let (mut stdin_tx, stdin_rx) = tokio::io::duplex(4096);
        let (stdout_tx, stdout_rx) = tokio::io::duplex(4096);
        let (daemon_rx, mut daemon_in) = tokio::io::duplex(4096);
        let (mut daemon_out, daemon_tx) = tokio::io::duplex(4096);

        // Client sends a request and immediately closes stdin (EOF)
        stdin_tx
            .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":42,\"method\":\"ping\"}\n")
            .await
            .unwrap();
        stdin_tx.flush().await.unwrap();
        drop(stdin_tx);

        // Daemon reads request slowly and replies AFTER stdin closed
        let daemon_task = tokio::spawn(async move {
            let mut lines = BufReader::new(&mut daemon_in).lines();
            let req_line = lines.next_line().await.unwrap().unwrap();
            assert!(req_line.contains("\"id\":42"));
            tokio::time::sleep(Duration::from_millis(60)).await;
            daemon_out
                .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":42,\"result\":{}}\n")
                .await
                .unwrap();
            daemon_out.flush().await.unwrap();
        });

        let outcome = proxy_session_loop(
            stdin_rx,
            stdout_tx,
            Some((daemon_tx, daemon_rx)),
            Duration::from_secs(2),
            || Box::pin(async { None }),
        )
        .await;

        assert_eq!(outcome, ProxyOutcome::CleanEof);
        daemon_task.await.unwrap();

        let mut out_lines = BufReader::new(stdout_rx).lines();
        let resp_line = out_lines.next_line().await.unwrap().unwrap();
        let resp_val: Value = serde_json::from_str(&resp_line).unwrap();
        assert_eq!(resp_val["id"], 42);
    }

    #[tokio::test]
    async fn test_proxy_emits_pending_errors_and_reconnects_mid_session() {
        let (mut stdin_tx, stdin_rx) = tokio::io::duplex(4096);
        let (stdout_tx, stdout_rx) = tokio::io::duplex(4096);
        let (d1_rx, mut d1_in) = tokio::io::duplex(4096);
        let (d1_out, d1_tx) = tokio::io::duplex(4096);

        let reconnect_calls = Arc::new(AtomicUsize::new(0));
        let reconnect_calls_clone = reconnect_calls.clone();

        // Spawn fake daemon #1: reads init + request 1, then crashes without replying to request 1
        tokio::spawn(async move {
            let mut lines = BufReader::new(&mut d1_in).lines();
            let _ = lines.next_line().await; // initialize
            let _ = lines.next_line().await; // ping id=1
            drop(d1_out); // abrupt disconnect
        });

        // Prepare fake daemon #2 for reconnect
        let (d2_rx, mut d2_in) = tokio::io::duplex(4096);
        let (mut d2_out, d2_tx) = tokio::io::duplex(4096);
        let d2_pair = Arc::new(tokio::sync::Mutex::new(Some((d2_tx, d2_rx))));
        tokio::spawn(async move {
            let mut lines = BufReader::new(&mut d2_in).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let v: Value = serde_json::from_str(&line).unwrap();
                let resp = json!({ "jsonrpc": "2.0", "id": v["id"], "result": {} });
                let out = serde_json::to_string(&resp).unwrap() + "\n";
                d2_out.write_all(out.as_bytes()).await.unwrap();
                d2_out.flush().await.unwrap();
            }
        });

        let proxy_task = tokio::spawn(async move {
            proxy_session_loop(
                stdin_rx,
                stdout_tx,
                Some((d1_tx, d1_rx)),
                Duration::from_secs(2),
                move || {
                    let calls = reconnect_calls_clone.clone();
                    let pair = d2_pair.clone();
                    Box::pin(async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        pair.lock().await.take()
                    })
                },
            )
            .await
        });

        stdin_tx
            .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":0,\"method\":\"initialize\",\"params\":{}}\n")
            .await
            .unwrap();
        // Reply for initialize isn't sent by d1 (we want id=0 and id=1 pending when d1 drops!)
        stdin_tx
            .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n")
            .await
            .unwrap();
        stdin_tx.flush().await.unwrap();

        tokio::time::sleep(Duration::from_millis(80)).await;
        // Send request 2 after reconnect
        stdin_tx
            .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}\n")
            .await
            .unwrap();
        stdin_tx.flush().await.unwrap();
        drop(stdin_tx);

        let outcome = proxy_task.await.unwrap();
        assert_eq!(outcome, ProxyOutcome::CleanEof);
        assert_eq!(reconnect_calls.load(Ordering::SeqCst), 1);

        let mut out_lines = BufReader::new(stdout_rx).lines();
        let mut received = Vec::new();
        while let Ok(Some(line)) = out_lines.next_line().await {
            received.push(serde_json::from_str::<Value>(&line).unwrap());
        }
        assert!(received.iter().any(|v| v["id"] == 1 && v["error"]["code"] == -32000));
        assert!(received.iter().any(|v| v["id"] == 2 && v.get("result").is_some()));
        assert!(!received.iter().any(|v| v["id"] == REINIT_SENTINEL_ID));
    }

    #[tokio::test]
    async fn test_proxy_reports_disconnect_when_reconnect_exhausted() {
        let (mut stdin_tx, stdin_rx) = tokio::io::duplex(4096);
        let (stdout_tx, stdout_rx) = tokio::io::duplex(4096);
        let (d1_rx, mut d1_in) = tokio::io::duplex(4096);
        let (d1_out, d1_tx) = tokio::io::duplex(4096);

        tokio::spawn(async move {
            let mut lines = BufReader::new(&mut d1_in).lines();
            let _ = lines.next_line().await;
            drop(d1_out);
        });

        let proxy_task = tokio::spawn(async move {
            proxy_session_loop(
                stdin_rx,
                stdout_tx,
                Some((d1_tx, d1_rx)),
                Duration::from_secs(2),
                || Box::pin(async { None }),
            )
            .await
        });

        stdin_tx
            .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"ping\"}\n")
            .await
            .unwrap();
        stdin_tx.flush().await.unwrap();

        let outcome = proxy_task.await.unwrap();
        assert_eq!(outcome, ProxyOutcome::DisconnectedMidSession);
        drop(stdin_tx);

        let mut out_lines = BufReader::new(stdout_rx).lines();
        let err_line = out_lines.next_line().await.unwrap().unwrap();
        let err_val: Value = serde_json::from_str(&err_line).unwrap();
        assert_eq!(err_val["id"], 9);
        assert_eq!(err_val["error"]["code"], -32000);
    }
}
