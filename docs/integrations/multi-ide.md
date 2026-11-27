# Multi-IDE / Multi-CLI Usage

Agent Guidance MCP runs as a **detached, ref-counted singleton daemon** over a cross-platform IPC channel (Windows Named Pipe or Unix domain socket). The first process spawns the detached daemon (loading models in the background and binding the IPC endpoint); subsequent processes auto-detect the running daemon and connect as a lightweight stdio-to-IPC proxy (< 5 MB RAM). All connected IDEs share a single model instance and embedded web dashboard.

## How It Works

Each IDE registers `agent-guidance` in its MCP config:

```json
{
  "mcpServers": {
    "agent-guidance": {
      "command": "agent-guidance",
      "args": []
    }
  }
}
```

### Connection Flow

```
IDE 1 → agent-guidance (PROXY) ── stdin/stdout ──┐
                                                 ▼
IDE 2 → agent-guidance (PROXY) ── IPC Pipe/Sock ─► DETACHED DAEMON (shared models + dashboard)
                                                 ▲
IDE 3 → agent-guidance (PROXY) ── stdin/stdout ──┘
```

| Step | What happens |
|---|---|
| IDE 1 launches | No IPC endpoint exists → spawns **detached daemon** (non-blocking < 2ms handshake, background model auto-warmup) and connects as **proxy** |
| IDE 2 launches | IPC endpoint exists → connects immediately as **proxy** (< 5 MB RAM), forwards stdio↔IPC |
| IDE 3, 4, ... | Same as IDE 2 — all share the daemon's models, 32-worker pool, and 2-permit ML queue |
| IDE 1 exits | Daemon continues serving remaining connections without fate-sharing |
| All IDEs disconnect | Ref count hits 0 → IDE Process Detector checks 26 IDE binaries → once 0 IDEs remain, **60s `Instant`-based idle cooldown** runs → clean exit |

### Models & Cache (shared)

| Component | Shared? | Notes |
|---|---|---|
| BERT embedding model | Shared (`OnceLock`) | `intfloat/multilingual-e5-small` (384-dim), ~118 MB RSS, loaded in background |
| Cross-encoder | Shared (`OnceLock`) | `cross-encoder/ms-marco-MiniLM-L-6-v2` (512-token truncation, pair encoding), ~80 MB RSS |
| Passage embeddings (629 embedded skills) | Shared (`OnceLock<RwLock<...>>`) | Instant ~0.1ms load from precomputed binary; local warmup persisted to `~/.agent-guidance/passage_vectors.bin` |
| ServerState (workflow stage, plan) | Per connection | Each IDE session has isolated state |
| Usage telemetry | Centralized | `~/.agent-guidance/usage.db` (SQLite WAL + `busy_timeout`) |

### IPC Endpoint Paths

- **Windows**: `\\.\pipe\agent-guidance-mcp`
- **Linux / macOS**: `~/.cache/agent-guidance/mcp.sock`

Created when the daemon starts and cleaned up on shutdown. Parent directories are auto-created.

## Non-Blocking Auto-Warmup & Latency

MCP `initialize` returns immediately (< 2ms) while models warm up on a background thread; precomputed vectors for all 629 skills load in ~0.1ms:

| Tool | First call latency | Subsequent |
|---|---|---|
| **initialize** | **< 2ms** (background auto-warmup) | < 2ms |
| `task_pipeline` | Fast heuristic fallback while warming, then ~50–200ms neural | ~50–200ms |
| `guidance(search)` | Instant precomputed cache + neural rerank once warm | ~50–200ms |
| All other tools | < 5–50ms (SQLite FTS5 / AST) | < 5–50ms |

## Benefits

- **Single model in RAM**: ~200 MB total regardless of IDE count (or ~15 MB per client when pointing to a Remote ML Worker)
- **Instant reconnect**: Subsequent IDEs connect in ~0.1ms with zero cold start
- **Zero fate-sharing**: Closing one IDE never tears down the shared daemon for other open IDEs
- **IDE-aware idle cleanup**: 60s idle cooldown (only after all 26 monitored IDE binaries exit) prevents premature model eviction and zombie processes

## RAM Usage Estimates (Local Standalone Mode)

| Setup | Without daemon | With daemon |
|---|---|---|
| 1 IDE | ~200 MB | ~200 MB |
| 2 IDEs | ~400 MB | **~200 MB** |
| 3 IDEs | ~600 MB | **~200 MB** |
| N IDEs | N × ~200 MB | **~200 MB** |
