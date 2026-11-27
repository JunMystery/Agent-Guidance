# Development Guide

[Back to README](../README.md)

This project is a high-performance **100% Native Rust 2024 Edition** MCP server exposing Agent Guidance MCP over cross-platform Named Pipe (Windows) and Unix domain socket (macOS/Linux) daemon transport.

## Setup & Build

Build the server using Rust 2024 (`cargo`):

```bash
cargo build
```

Build optimized release binary:

```bash
cargo build --release
```

## Running the Server

Run directly with Cargo (auto-detects daemon mode):

```bash
cargo run -- --setup
```

Start native web dashboard server:

```bash
cargo run -- --dashboard
```

Force daemon or client mode:

```bash
cargo run -- --force-daemon   # start as daemon (skip auto-detect)
cargo run -- --force-client   # connect to existing daemon
```

## Testing

Run the automated Rust unit/integration test suites, linter, and packaging checks:

```bash
cargo test --bin agent-guidance
cargo test --tests
cargo clippy --bin agent-guidance --tests
node tests/tier5_adversarial_frontend_test.mjs
npm test --prefix packaging/npm
```

Run a whitespace and line-ending check before committing (excluding vendored `skills/` content if desired):

```bash
git diff --check -- ':!skills/'
```

## Binary Signing & Security Provenance

Release binaries are digitally signed and fingerprinted to prevent false-positive antivirus blocks (Windows Defender / SmartScreen / macOS Gatekeeper).

### Inspect Binary Fingerprint
```bash
agent-guidance --fingerprint
# or: agent-guidance --verify-signature
```
Displays executable path, cryptographic SHA-256 digest, Git commit, architecture target, and platform signature status.

### Windows Authenticode Signing
```powershell
powershell -ExecutionPolicy Bypass -File scripts/sign-binary.ps1 -Target "target/release/agent-guidance.exe"
```
Signs the binary with SHA-256 and RFC-3161 timestamping, enrolling the publisher into `Cert:\CurrentUser\TrustedPublisher`.

### Unix & macOS Code Signing
```bash
bash scripts/sign-binary.sh target/release/agent-guidance
```
Applies hardened runtime (`--options runtime`) codesign on macOS and generates SHA-256/512 and GPG signatures on Linux.

## Project Structure

```text
Agent-Guidance/
├── docs/                    # Maintainer and user documentation
├── packaging/               # NPM launcher and Homebrew packaging
├── references/              # Orchestration patterns and reference docs
├── scripts/                 # Installers, uninstallers, signing, version sync
├── skills/                  # 629 embedded on-demand skill capsules
├── tests/                   # Rust integration and JS frontend adversarial tests
├── src/                     # Rust source code
│   ├── main.rs              # Binary entrypoint — CLI flags, auto-detect daemon/proxy
│   ├── cli/                 # CLI subcommands (cleanup, setup-server, status/stats)
│   ├── client/              # Remote ML worker HTTP client & SQLite cache
│   ├── config/              # TOML configuration store & hot-reload watcher
│   ├── daemon/              # Detached singleton daemon (Named Pipe / Unix socket, 60s cooldown, tray)
│   ├── catalog/             # Embedded 629-skill catalog, blueprints, language rules
│   ├── context/             # AST engine, complexity/test-safety analyzers, SQLite GraphRAG, HNSW, LSP
│   ├── dashboard/           # Embedded HTTP usage dashboard & REST API
│   ├── dashboard_src/       # Dashboard frontend SPA (HTML5, Vanilla ES Modules, CSS)
│   ├── mcp/                 # MCP protocol engine (router/, tools/, state/, snapshots, db/, config/)
│   ├── ml/                  # Candle BERT / ONNX embeddings (no ndarray), cross-encoder, remote worker
│   └── optimizer/           # Token compressor, AST skeletonizer & zoom slicer
├── Cargo.toml               # Rust package metadata (v2.0.3)
├── AGENTS.md                # Agent execution protocol (Codex / universal)
├── CLAUDE.md                # Agent execution protocol (Claude Code)
├── GEMINI.md                # Agent execution protocol (Gemini / Antigravity)
├── PROJECT-STANDARDS.md     # Project-specific agent standards
├── CHANGELOG.md             # Version history
└── README.md                # Compact landing page
```

## Core Source Files

| Module | Path | Role |
|---|---|---|
| Entrypoint | `src/main.rs` | CLI flags (`--setup`, `--upgrade`, `--cleanup`, `--dashboard`, `--server`, `--uninstall`), daemon/proxy dispatch |
| CLI & Server Setup | `src/cli/` | `--cleanup --retention-days`, `--setup-server` (`AGENT_GUIDANCE_API_KEY` templates), `--set-server`, `--stats` |
| Daemon & Tray | `src/daemon/` | Named Pipe / Unix socket lifecycle, 26-IDE process detector, 60s `Instant`-based idle cooldown, cross-platform tray |
| MCP Router | `src/mcp/router/` | JSON-RPC dispatcher, tool schemas (`tools.rs`), resource router (`resources.rs`), initialize handshake |
| MCP Tools | `src/mcp/tools/` | 6 tool dispatchers (`task_pipeline`, `select_skills`, `guidance`, `project_context`, `workflow_gate`, `session_continuity`) |
| MCP State & Snapshots | `src/mcp/state/`, `src/mcp/snapshots.rs` | `ServerState` priority gate, stage matrix, circuit breaker, session pre-edit snapshots |
| Embeddings | `src/ml/embeddings/` | Candle BERT / ONNX embedding engine (pure `candle`/`ort` tensors, `ndarray` removed) + local warmup cache (`~/.agent-guidance/passage_vectors.bin`) |
| Reranker | `src/ml/cross_encoder.rs`, `src/ml/llm_selector.rs` | MiniLM cross-encoder reranker (512-token truncation, pair encoding) & candidate selector |
| Remote ML Worker | `src/ml/worker/` | HTTP worker daemon (`vectors.bin`, `AGENT_GUIDANCE_API_KEY`, non-loopback key enforcement, 1 MB body cap) |
| Context & GraphRAG | `src/context/` | Workspace scanner, AST complexity/test-safety checks, SQLite FTS5/GraphRAG indexer, HNSW, LSP |
| Optimizer | `src/optimizer/` | Language-aware token compressor, AST skeletonizer, heuristic zoom slicer |

## Documentation & Skill Catalog Sync

- Keep `README.md` compact and link to detailed docs.
- Keep `docs/skills/SKILLS_OVERVIEW.md` synchronized with `skills/*/SKILL.md` frontmatter.
- Whenever skills are added, removed, or modified under `skills/`, regenerate `src/ml/precomputed_vectors.bin` and `src/ml/precomputed_manifest.json` using:

```bash
./target/release/agent-guidance --reindex-skills
```

- CI (`.github/workflows/ci.yml`) and Release (`.github/workflows/release.yml`) workflows automatically fail if `precomputed_manifest.json` skill count differs from `skills/` directory count.
- Add new user-facing reference docs under `docs/`.
- Use relative Markdown links so GitHub and IDE previews can open files directly.

## Version Bump

Run the single-source-of-truth synchronizer script when releasing a new version:

```bash
python scripts/sync-version.py X.Y.Z
```

This updates `Cargo.toml`, `Cargo.lock`, `build.rs`, `package.json`, `packaging/npm/package.json`, `packaging/homebrew/agent-guidance.rb`, `scripts/install.ps1`, `scripts/install.sh`, and `src/dashboard_src/package.json`. Update `.github/workflows/release.yml` default tag example if desired.

Files that auto-follow via `env!("CARGO_PKG_VERSION")` (no manual change):
- `src/main.rs`, `src/mcp/router/mod.rs`

Procedure: `python scripts/sync-version.py X.Y.Z` → `cargo build --release` → `git tag vX.Y.Z` → `git push --tags`.

## Related Docs

- [Installation](installation.md)
- [Client Setup](setup/client-configuration.md)
- [MCP Surface](reference/mcp-surface.md)
