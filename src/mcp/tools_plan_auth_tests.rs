use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::mcp::state::ServerState;
use crate::mcp::tools::gate_approval::handle_approve_plan;
use crate::mcp::tools::gate_plan_parser::{extract_paths_from_plan_content, find_plan_artifact};

static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);
static BRAIN_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn create_temp_dir() -> PathBuf {
    let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("ag_plan_auth_test_{}_{}", std::process::id(), id));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn test_extract_paths_from_markdown_table_and_list() {
    let temp = create_temp_dir();
    let proj_path = &temp;

    let markdown = format!(
        r#"
# Implementation Plan

## 1. User Review Required / Architecture Design
Some overview text here with false alarms like `cargo test` and `v2.0.0`.

## 2. Proposed Changes
### Core Engine
| File Path | Status | Notes |
|---|---|---|
| `src/domain/model.rs` | PASSED | New domain model |
| `src/entrypoint/handler.rs` | PASSED | Dispatcher |

### Documentation
- [mcp-surface.md](file://{}/docs/reference/mcp-surface.md): Update documentation
- `docs/usage.md`: Add examples
- Ignore `implementation_plan.md` and `https://github.com/org/repo/file.rs`
"#,
        proj_path.display()
    );

    let paths = extract_paths_from_plan_content(&markdown, proj_path);
    assert_eq!(
        paths,
        vec![
            "src/domain/model.rs".to_string(),
            "src/entrypoint/handler.rs".to_string(),
            "docs/reference/mcp-surface.md".to_string(),
            "docs/usage.md".to_string(),
        ]
    );
    let _ = fs::remove_dir_all(&temp);
}

fn canonical_path(p: &Path) -> PathBuf {
    fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

#[test]
fn test_plan_artifact_discovery_multitier() {
    let temp = create_temp_dir();
    let proj = &temp;

    // 1. Initially none found
    assert!(find_plan_artifact(proj, None, None).is_none());

    // 2. Custom path takes top priority
    let custom_file = proj.join("custom_plan.md");
    fs::write(&custom_file, "# Custom").unwrap();
    let found = find_plan_artifact(proj, Some("custom_plan.md"), None);
    assert_eq!(found.as_deref().map(canonical_path), Some(canonical_path(&custom_file)));

    // 3. Fallback to workspace implementation_plan.md
    let ws_plan = proj.join("implementation_plan.md");
    fs::write(&ws_plan, "# Workspace Plan").unwrap();
    let found_ws = find_plan_artifact(proj, None, None);
    assert_eq!(found_ws.as_deref().map(canonical_path), Some(canonical_path(&ws_plan)));

    // 4. Fallback to .agent-context
    let ctx_dir = proj.join(".agent-context");
    fs::create_dir_all(&ctx_dir).unwrap();
    let ctx_plan = ctx_dir.join("implementation_plan.md");
    fs::write(&ctx_plan, "# Context Plan").unwrap();
    fs::remove_file(proj.join("implementation_plan.md")).unwrap();
    let found_ctx = find_plan_artifact(proj, None, None);
    assert_eq!(found_ctx.as_deref().map(canonical_path), Some(canonical_path(&ctx_plan)));

    // 5. Fallback to brain dir when workspace has no plans
    fs::remove_file(ctx_dir.join("implementation_plan.md")).unwrap();
    let brain_dir = temp.join("mock_brain");
    let session_dir = brain_dir.join("sess_xyz");
    fs::create_dir_all(&session_dir).unwrap();
    let brain_plan = session_dir.join("implementation_plan.md");
    fs::write(&brain_plan, "# Brain Plan").unwrap();
    {
        let _guard = BRAIN_ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("AGY_BRAIN_DIR", &brain_dir);
        }
        let found_brain = find_plan_artifact(proj, None, Some("sess_xyz"));
        assert_eq!(found_brain.as_deref().map(canonical_path), Some(canonical_path(&brain_plan)));
        unsafe {
            std::env::remove_var("AGY_BRAIN_DIR");
        }
    }

    let _ = fs::remove_dir_all(&temp);
}

#[test]
fn test_approve_plan_auto_auth_end_to_end() {
    let temp = create_temp_dir();
    let proj = &temp;

    let plan_content = r#"
# Plan
## Proposed Changes
| File Path | Description |
|---|---|
| `src/domain/user.rs` | Domain entity |
| `src/adapter/repo.rs` | Repository implementation |
"#;
    fs::write(proj.join("implementation_plan.md"), plan_content).unwrap();

    let mut state = ServerState::new();
    let args = json!({
        "justification": "Implement domain and repository"
    });

    let res = handle_approve_plan(&args, &mut state, true, None, proj);
    assert!(res.contains("PASSED"));
    assert!(res.contains("Plan Approved: true"));
    assert!(res.contains("Plan Artifact Auto-Auth: 2 file(s) automatically discovered"));
    assert!(res.contains("src/domain/user.rs"));
    assert!(res.contains("src/adapter/repo.rs"));
    assert!(state.edit_authorized);
    assert_eq!(state.workflow_stage, "Build");

    let _ = fs::remove_dir_all(&temp);
}

#[test]
fn test_approve_plan_auto_auth_deduplication() {
    let temp = create_temp_dir();
    let proj = &temp;

    let plan_content = r#"
## Proposed Changes
- `src/main.rs`: Entrypoint
- `src/domain/user.rs`: Model
"#;
    fs::write(proj.join("implementation_plan.md"), plan_content).unwrap();

    let mut state = ServerState::new();
    state.queue_deferred_authorization(&["src/lib.rs".to_string()]);

    let args = json!({
        "relative_paths": ["src/main.rs"],
        "justification": "Multi-tier deduplication test"
    });

    let res = handle_approve_plan(&args, &mut state, true, None, proj);
    assert!(res.contains("Batch Pre-Authorization: 3 file(s) processed"));
    assert!(res.contains("Plan Artifact Auto-Auth: 1 file(s) automatically discovered"));
    assert!(res.contains("src/main.rs"));
    assert!(res.contains("src/lib.rs"));
    assert!(res.contains("src/domain/user.rs"));

    let _ = fs::remove_dir_all(&temp);
}

#[test]
fn test_approve_plan_auto_auth_modularity_guard() {
    let temp = create_temp_dir();
    let proj = &temp;

    let plan_content = r#"
## Proposed Changes
| File | Action |
|---|---|
| `src/shared_services.rs` | Compound plural anti-pattern |
"#;
    fs::write(proj.join("implementation_plan.md"), plan_content).unwrap();

    let mut state = ServerState::new();
    let args = json!({
        "justification": "Test compound plural guard"
    });

    let res = handle_approve_plan(&args, &mut state, true, None, proj);
    assert!(res.contains("PASSED"));
    assert!(res.contains("DOMAIN_COHESION_ADVISORY"));
    assert!(res.contains("shared_services.rs"));

    let _ = fs::remove_dir_all(&temp);
}

#[test]
fn test_extract_paths_strips_windows_file_uri_and_line_anchors() {
    let temp = create_temp_dir();
    let markdown = r#"
## Proposed Changes
- [main.rs](file:///d:/Git/Agent-Guidance/src/main.rs#L10-L25)
- `src/mcp/tools/gate.rs?foo=bar#L5`
"#;
    let paths = extract_paths_from_plan_content(markdown, Path::new("d:/Git/Agent-Guidance"));
    assert_eq!(
        paths,
        vec![
            "src/main.rs".to_string(),
            "src/mcp/tools/gate.rs".to_string(),
        ]
    );
    let _ = fs::remove_dir_all(&temp);
}

#[test]
fn test_find_plan_artifact_rejects_traversal_and_non_markdown_custom_paths() {
    let base = create_temp_dir();
    let repo = base.join("repo");
    let ext = base.join("outside");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&ext).unwrap();
    let outside_md = ext.join("secret.md");
    let non_md = repo.join("Cargo.toml");
    fs::write(&outside_md, "# Secret").unwrap();
    fs::write(&non_md, "[package]").unwrap();

    assert!(find_plan_artifact(&repo, Some("../outside/secret.md"), None).is_none());
    assert!(find_plan_artifact(&repo, Some(outside_md.to_str().unwrap()), None).is_none());
    assert!(find_plan_artifact(&repo, Some("Cargo.toml"), None).is_none());

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn test_plan_parser_skips_code_blocks_excluded_sections_and_prose_js_tokens() {
    let base = create_temp_dir();
    let repo = base.join("app");
    fs::create_dir_all(&repo).unwrap();
    fs::write(repo.join("Cargo.toml"), "[package]\n").unwrap();

    let markdown = r#"
# Implementation Plan

## Proposed Changes
We use `next.js`, `vue.js`, and `node.js` in prose, plus `Cargo.toml`.
- `src/components/Button.tsx`
- src/views/Dashboard.vue: Plain path under Proposed Changes
- `scripts/deploy.ps1`
- `d:/ag_plan_auth_other/src/should_not_strip.rs`

```rust
// Inside fenced code block: must NOT be extracted
let fake = "`src/fake_from_codeblock.rs`";
```

## Verification Plan
- Run tests in `src/verify_only.rs`

## Out of Scope
- `src/out_of_scope.rs`
"#;

    let paths = extract_paths_from_plan_content(markdown, &repo);
    assert!(paths.contains(&"Cargo.toml".to_string()));
    assert!(paths.contains(&"src/components/Button.tsx".to_string()));
    assert!(paths.contains(&"src/views/Dashboard.vue".to_string()));
    assert!(paths.contains(&"scripts/deploy.ps1".to_string()));
    assert!(paths.contains(&"d:/ag_plan_auth_other/src/should_not_strip.rs".to_string()));
    assert!(!paths.contains(&"next.js".to_string()));
    assert!(!paths.contains(&"vue.js".to_string()));
    assert!(!paths.contains(&"node.js".to_string()));
    assert!(!paths.contains(&"src/fake_from_codeblock.rs".to_string()));
    assert!(!paths.contains(&"src/verify_only.rs".to_string()));
    assert!(!paths.contains(&"src/out_of_scope.rs".to_string()));

    // Prefix stripping must respect `/` boundary (`d:/ag_plan_auth` must not strip `d:/ag_plan_auth_other/...`)
    let boundary_paths = extract_paths_from_plan_content(
        "## Proposed Changes\n- `d:/ag_plan_auth_other/src/keep.rs`\n",
        Path::new("d:/ag_plan_auth"),
    );
    assert_eq!(boundary_paths, vec!["d:/ag_plan_auth_other/src/keep.rs".to_string()]);

    // UTF-8 prefix slicing safety
    let utf8_paths = extract_paths_from_plan_content(
        "## Proposed Changes\n- `src/tài_liệu_tiếng_việt.md`\n",
        Path::new("d:/Dự án rất dài hơn path"),
    );
    assert_eq!(utf8_paths, vec!["src/tài_liệu_tiếng_việt.md".to_string()]);

    // Invalid AGY_CONVERSATION_ID with path traversal must be ignored
    let brain_dir = base.join("brain_sec");
    fs::create_dir_all(&brain_dir).unwrap();
    fs::write(base.join("implementation_plan.md"), "# Secret").unwrap();
    {
        let _guard = BRAIN_ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("AGY_BRAIN_DIR", &brain_dir);
            std::env::set_var("AGY_CONVERSATION_ID", "..");
        }
        assert!(find_plan_artifact(&repo, None, None).is_none());
        unsafe {
            std::env::remove_var("AGY_CONVERSATION_ID");
            std::env::remove_var("AGY_BRAIN_DIR");
        }
    }

    let _ = fs::remove_dir_all(&base);
}
