# Repo Map For Agents

Use this map before changing the repository. Prefer the smallest relevant source file and avoid broad documentation moves unless explicitly requested.

## Root & IDE Instruction Files

- `AGENTS.md`: OpenAI Codex and universal agent execution protocol.
- `CLAUDE.md`: Claude Code execution protocol.
- `GEMINI.md`: Gemini Code Assist, Gemini CLI, and Antigravity execution protocol.
- `.github/copilot-instructions.md`: GitHub Copilot instructions.
- `.cursor/rules/agent-guidance.mdc`: Cursor rule file with frontmatter.
- `.agents/skills/agent-guidance/SKILL.md` and `.claude/skills/agent-guidance/SKILL.md`: Workspace skill definitions for the protocol.

## Core Sources

| Path | Description |
|---|---|
| `src/` | Rust 2024 source code for the MCP server (v2.0.3) |
| `src/main.rs` | Binary entrypoint — daemon/proxy auto-detection, CLI flags |
| `src/cli/` | CLI handlers (`cleanup.rs`, `setup_server.rs`, server/stats/delete-skill commands) |
| `src/client/` | Remote ML worker HTTP client (`ureq`) and SQLite cache |
| `src/config/` | TOML configuration schema, store, and live file watcher |
| `src/daemon/` | Detached singleton daemon (Windows Named Pipe / Unix domain socket, 26-IDE detector, 60s idle cooldown, system tray) |
| `src/mcp/` | MCP protocol engine (`router/`, `tools/`, `state/`, `snapshots.rs`, `learnings/`, `db/`, `config/`, `fingerprint.rs`) |
| `src/ml/` | ML models (`embeddings/` with `~/.agent-guidance/passage_vectors.bin`, `cross_encoder.rs`, `llm_selector.rs`, `worker/`) |
| `src/catalog/` | 629 embedded skills (`rust_embed`), blueprints, language rules, section slicer |
| `src/context/` | Workspace scanner, Tree-sitter AST engine, complexity/test-safety analyzers, SQLite FTS5/GraphRAG indexer, HNSW, LSP |
| `src/optimizer/` | Language-aware token compressor, AST skeletonizer, heuristic zoom slicer |
| `src/dashboard/` | Embedded `tiny_http` dashboard backend, Origin/Bearer auth, REST APIs |
| `src/dashboard_src/` | Dashboard SPA frontend (HTML5, Vanilla ES Modules, CSS) |
| `packaging/npm/` | Cross-platform NPX/NPM runner and launcher tests |
| `Cargo.toml` | Rust package metadata (v2.0.3; pure `candle`/`ort` tensors, no `ndarray`) |
| `skills/` | 629 embedded on-demand skill capsules (`skills/<name>/SKILL.md`) |
| `scripts/` | Installers, uninstallers, binary signing, and `sync-version.py` |
| `docs/` | User, setup, architecture, and maintainer documentation |
| `tests/` | Rust integration tests and frontend adversarial tests (`tier5_adversarial_frontend_test.mjs`) |

## Common Workflows

- Updating a skill: edit `skills/<name>/SKILL.md` and run `agent-guidance --reindex-skills` to keep `src/ml/precomputed_vectors.bin` and `src/ml/precomputed_manifest.json` in sync.
- Adding/updating MCP tools: edit `src/mcp/tools/` and register schemas in `src/mcp/router/tools.rs`.
- Updating ML models or worker endpoints: edit `src/ml/embeddings/`, `src/ml/cross_encoder.rs`, or `src/ml/worker/`.
- Releasing: `python scripts/sync-version.py X.Y.Z` → `cargo build --release` → `git tag vX.Y.Z` → `git push --tags`.

## Verification

Use the narrowest checks that prove the change:

```bash
cargo test --bin agent-guidance
cargo test --tests
cargo clippy --bin agent-guidance --tests
node tests/tier5_adversarial_frontend_test.mjs
npm test --prefix packaging/npm
git diff --check -- ':!skills/'
```
