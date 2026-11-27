use serde_json::Value;

use crate::context::cache::project_snapshot;
use crate::mcp::state::ServerState;
use super::{detect_project_architecture, detect_project_path, ensure_indexed, ensure_not_cancelled};
pub use super::pipeline_fast_path::infer_or_normalize_phase;
use super::pipeline_fast_path::{build_fast_path_response, is_minor_task};

pub(crate) fn handle(
    arguments: Value,
    state: &mut ServerState,
) -> Result<String, (i32, String)> {
    ensure_not_cancelled(state)?;
    let raw_task = arguments
        .get("task")
        .and_then(|t| t.as_str())
        .unwrap_or("general task");
    let task = if raw_task.trim().is_empty() {
        "general task"
    } else {
        raw_task.trim()
    };
    let proj_path_arg = arguments
        .get("project_path")
        .and_then(|p| p.as_str())
        .unwrap_or(".");
    let proj_path = detect_project_path(proj_path_arg, state);
    if !state.workspace_roots.is_empty() && !state.is_path_inside_workspace(&proj_path) {
        tracing::warn!(
            "Rejected out-of-workspace project_path '{}' (workspace_roots: {:?})",
            proj_path.display(),
            state.workspace_roots
        );
        return Err((
            -32602,
            format!(
                "Project path '{}' is outside active workspace roots ({:?}).",
                proj_path.display(),
                state.workspace_roots
            ),
        ));
    }
    state.update_project_path(&proj_path);
    let had_prior_plan_turn = state.workflow_stage == "Plan"
        && (state.user_intent_summary.is_some() || !state.recent_tasks.is_empty());
    state.priority_gate_pass();
    if state.workflow_stage == "Context" {
        state.workflow_stage = "Plan".to_string();
    }

    let task_intent = super::guidance_intent::analyze_query_intent(task, &[]);
    let norm_task = task.to_lowercase().replace('-', "_");
    let is_gen_ui_task = task_intent.slash_commands.iter().any(|c| c == "generative_ui")
        || task_intent.mentioned_skills.iter().any(|s| s == "generative_ui")
        || crate::catalog::language_detector::contains_word(&norm_task, "generative_ui");

    // Fast-Path Exemption for Minor Tasks (typos, comments, docs, single-file patches)
    if !is_gen_ui_task && is_minor_task(task, &arguments) {
        let _ = ensure_indexed(&proj_path);
        let detected_arch = detect_project_architecture(&proj_path);
        state.priority_gate_pass();
        state.workflow_stage = "Build".to_string();
        state.plan_approved = true;
        state.edit_authorized = false;
        state.is_fast_path = true;
        state.fast_path_files.clear();
        state.pending_skill_proposals.clear();
        state.clear_deferred_authorized_paths();
        state.verification_passed = false;
        state.verification_command = None;
        state.expected_output_keyword = None;
        state.active_architecture_pattern = Some(detected_arch.clone());
        state.user_intent_summary = Some(task.to_string());
        state.active_task = Some(task.to_string());
        state.active_phase = Some("build".to_string());
        state.push_recent_task(task);

        let target_paths = super::gate_edit::extract_target_paths(&arguments);
        if !target_paths.is_empty() {
            if super::gate_edit::enforce_edit_boundaries(&proj_path, &target_paths, false, state).is_none() {
                let _ = super::gate_edit_batch::handle_batch_authorize(
                    &proj_path,
                    &target_paths,
                    task,
                    &detected_arch,
                    false,
                    state,
                );
            }
        }

        let _ = state.save_to_dir(&proj_path);
        let fast_resp = build_fast_path_response(task, &proj_path);
        return Ok(fast_resp);
    }

    let raw_phase = arguments
        .get("phase")
        .and_then(|p| p.as_str());
    let (mut phase, mut display_phase) = match raw_phase {
        Some("plan") | None => {
            let inferred = infer_or_normalize_phase(raw_phase, task);
            let disp = inferred.clone();
            (inferred, disp)
        }
        Some(explicit) => {
            let normalized = infer_or_normalize_phase(raw_phase, task);
            (normalized, explicit.to_string())
        }
    };

    // Record active phase and auto-reset approval state when starting a new planning phase
    let is_same_task = state.user_intent_summary.as_deref() == Some(task);
    let task_lower = task.to_lowercase();
    let is_explicit_gui_approval = task_lower.contains("implementation_plan")
        || task_lower.contains("approved this document")
        || task_lower.contains("proceeded with implementation plan")
        || task_lower.contains("proceed with implementation plan");
    let is_approval_msg = (had_prior_plan_turn || is_explicit_gui_approval)
        && ServerState::is_gui_or_short_plan_approval(task);
    if is_approval_msg {
        phase = "build".to_string();
        display_phase = "build".to_string();
        state.active_phase = Some("build".to_string());
        state.workflow_stage = "Build".to_string();
        state.approve_plan();
        state.edit_authorized = false;
        state.pending_skill_proposals.clear();
        tracing::info!("Detected plan approval in task pipeline; transitioned to Build stage with plan_approved=true.");
    } else if !is_same_task
        || (phase == "plan"
            && state.workflow_stage != "Build"
            && state.workflow_stage != "Test_Recheck")
    {
        state.active_phase = Some(phase.clone());
        state.workflow_stage = "Plan".to_string();
        state.plan_approved = false;
        state.edit_authorized = false;
        state.verification_passed = false;
        state.verification_command = None;
        state.expected_output_keyword = None;
        state.pending_skill_proposals.clear();
        state.clear_deferred_authorized_paths();
        state.clear_fast_path();
        state.user_intent_summary = Some(task.to_string());
        state.active_task = Some(task.to_string());
        state.push_recent_task(task);
        tracing::info!(
            "Reset workflow stage to 'Plan' and plan_approved to false for new task pipeline execution."
        );
    } else {
        state.active_phase = Some(phase.clone());
        if (phase == "build" || phase == "fix" || phase == "refactor") && state.plan_approved {
            state.workflow_stage = "Build".to_string();
        }
        state.user_intent_summary = Some(task.to_string());
        state.active_task = Some(task.to_string());
        state.push_recent_task(task);
    }
    let effective_task = if is_approval_msg {
        state
            .active_task
            .as_deref()
            .or(state.user_intent_summary.as_deref())
            .unwrap_or(task)
            .to_string()
    } else {
        task.to_string()
    };

    // Fast-path file count: use existing code_graph.db count if present, else snapshot
    let file_count = {
        let db_path = proj_path.join(".agent-context").join("code_graph.db");
        if db_path.exists() {
            if let Ok(db) = crate::context::db::CodeGraphDb::open_read_only(&db_path) {
                db.conn
                    .query_row("SELECT COUNT(*) FROM files", [], |r| r.get::<_, i64>(0))
                    .map(|c| c as usize)
                    .unwrap_or(0)
            } else {
                0
            }
        } else {
            0
        }
    };
    let file_count = if file_count > 0 {
        file_count
    } else {
        project_snapshot(&proj_path).files.len()
    };
    ensure_not_cancelled(state)?;
    let _ = ensure_indexed(&proj_path);

    let detected_arch = detect_project_architecture(&proj_path);
    state.active_architecture_pattern = Some(detected_arch.clone());

    let core_rules_checklist = crate::catalog::rules::get_phase_rules(&phase);

    let dynamic_blueprint = crate::catalog::blueprint::generate_dynamic_blueprint(&proj_path, &effective_task, &detected_arch);
    let relevant_learnings = crate::mcp::learnings::get_semantic_relevant_learnings(&proj_path, &effective_task, 3, 0.82);

    let learnings_section = if relevant_learnings.is_empty() {
        String::new()
    } else {
        format!("\n\n## Project Memorized Learnings\n{}", relevant_learnings.join("\n"))
    };

    let blueprint_section = if dynamic_blueprint.trim().is_empty() {
        String::new()
    } else {
        format!("\n\n## Dynamic Split Blueprint\n{}", dynamic_blueprint)
    };

    let should_bundle_skills = !is_approval_msg && matches!(raw_phase, None | Some("plan"));
    let skills_section = if should_bundle_skills {
        match super::guidance_search::handle_search(&arguments, &effective_task.to_lowercase(), state) {
            Ok(out) => format!("\n\n{}", out),
            Err(_) => String::new(),
        }
    } else {
        String::new()
    };

    let next_step_prompt = if !skills_section.is_empty() {
        if skills_section.contains("SKILL_PROPOSAL: MANDATORY USER INTERACTION REQUIRED") {
            "-> NEXT STEP: Ask user via `ask_question` using the bundled Skill Proposal above, then invoke `select_skills(skills=[...], user_confirmed=true)`."
        } else {
            "-> NEXT STEP: Skill discovery complete (no confirmation needed). Proceed directly to task planning or codebase inspection (`project_context`)."
        }
    } else if phase == "plan" {
        "-> NEXT STEP: Call `guidance(operation=\"search\", query=\"...\")` FIRST to discover and propose required domain skills. When skills are proposed, ask user via `ask_question` before invoking `select_skills(skills=[...], user_confirmed=true)`. If no matching skills are found, proceed directly to task planning or codebase inspection (`project_context`) without asking."
    } else if phase == "build" || phase == "fix" || phase == "refactor" {
        "-> NEXT STEP: If codebase inspection is needed, use `project_context(operation=\"search\" | \"read\")`. When ready to write code, obtain authorization via `workflow_gate(action=\"authorize_edit\", ...)`."
    } else if phase == "test" {
        "-> NEXT STEP: Run verification commands with `guidance(operation=\"verify\", verification_command=\"...\", expected_output_keyword=\"...\")`."
    } else if phase == "review" {
        "-> NEXT STEP: Inspect changes with `project_context(operation=\"read\")` and review domain cohesion, function complexity, and static linter output."
    } else {
        "-> NEXT STEP: If codebase inspection is needed, use `project_context(operation=\"search\" | \"read\")`. Otherwise, proceed with task planning."
    };

    let indexing_suffix = if crate::context::graph_rag::jit_sync::is_indexing(&proj_path) {
        ", background GraphRAG indexing active"
    } else {
        ""
    };

    state.last_raw_baseline_tokens = Some(800);
    let _ = state.save_to_dir(&proj_path);
    Ok(format!(
        "# Task Pipeline Activated\n\nTask: {}\nActive Phase: {}\nProject: {} ({} files scanned{}){}{}\n\n## Architecture Guidance\n- Active Pattern: {}\n- Enforce: Organize modules by domain/feature, keep functions focused (<= 60 LOC), and treat 300 LOC as a soft cohesion threshold\n\n{}{}\n\nPriority Gate: PASSED\nStatus: Ready for execution.\n\n{}",
        effective_task,
        display_phase,
        proj_path.display(),
        file_count,
        indexing_suffix,
        learnings_section,
        blueprint_section,
        detected_arch,
        core_rules_checklist,
        skills_section,
        next_step_prompt
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infer_or_normalize_phase() {
        assert_eq!(infer_or_normalize_phase(Some("implement"), "any task"), "build");
        assert_eq!(infer_or_normalize_phase(Some("debug"), "any task"), "fix");
        assert_eq!(infer_or_normalize_phase(Some("verify"), "any task"), "test");
        assert_eq!(infer_or_normalize_phase(Some("audit"), "any task"), "review");

        // Auto inference from task when phase is plan or empty
        assert_eq!(infer_or_normalize_phase(Some("plan"), "Fix crash on startup"), "fix");
        assert_eq!(infer_or_normalize_phase(Some("plan"), "Sửa lỗi crash"), "fix");
        assert_eq!(infer_or_normalize_phase(Some("plan"), "Write unit test for auth"), "test");
        assert_eq!(infer_or_normalize_phase(Some("plan"), "Code review for security"), "review");
        assert_eq!(infer_or_normalize_phase(Some("plan"), "Refactor scanner into sub-modules"), "refactor");
        assert_eq!(infer_or_normalize_phase(Some("plan"), "Triển khai tính năng mới"), "build");

        // Explicit plan in task preserves plan
        assert_eq!(infer_or_normalize_phase(Some("plan"), "Lên kế hoạch sửa lỗi crash"), "plan");
        assert_eq!(infer_or_normalize_phase(Some("plan"), "Design architecture for payment"), "plan");
        assert_eq!(infer_or_normalize_phase(Some("plan"), "general planning task"), "plan");
    }

    #[test]
    fn test_task_pipeline_bundles_skill_discovery() {
        let temp_dir = std::env::temp_dir().join(format!("pipeline_bundle_test_{}", std::process::id()));
        let skill_dir = temp_dir.join(".agents").join("skills").join("sql-safety");
        let _ = std::fs::create_dir_all(&skill_dir);
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: sql-safety\ndescription: SQL safety rules.\n---\n# SQL Safety\n",
        ).unwrap();

        let mut state = ServerState::new();
        let res = handle(
            serde_json::json!({
                "task": "Use sql-safety to fix SQL queries",
                "project_path": temp_dir.to_str().unwrap(),
                "phase": "plan"
            }),
            &mut state,
        );
        let _ = std::fs::remove_dir_all(&temp_dir);
        let out = res.unwrap();
        assert!(out.contains("Skill Search Results"));
        assert!(out.contains("sql-safety"));
        assert!(!out.contains("Call `guidance(operation=\"search\""));
    }

    #[test]
    fn test_pipeline_project_switch_does_not_leak_had_prior_plan_turn() {
        let base = std::env::temp_dir().join(format!("ag_pipe_leak_{}", std::process::id()));
        let repo_a = base.join("repo_a");
        let repo_b = base.join("repo_b");
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::create_dir_all(&repo_a);
        let _ = std::fs::create_dir_all(&repo_b);

        let mut state = ServerState::new();
        handle(
            serde_json::json!({
                "task": "Plan feature A",
                "project_path": repo_a.to_str().unwrap(),
                "phase": "plan"
            }),
            &mut state,
        )
        .unwrap();
        assert_eq!(state.workflow_stage, "Plan");

        // Switching to repo_b with short phrase "ok" must NOT treat repo_a's Plan turn as approval
        handle(
            serde_json::json!({
                "task": "ok",
                "project_path": repo_b.to_str().unwrap(),
                "phase": "plan"
            }),
            &mut state,
        )
        .unwrap();
        assert!(!state.plan_approved);
        assert_eq!(state.workflow_stage, "Plan");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn test_pipeline_new_task_with_build_phase_resets_prior_state() {
        let temp_dir = std::env::temp_dir().join(format!("ag_pipe_reset_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        let _ = std::fs::create_dir_all(&temp_dir);

        let mut state = ServerState::new();
        state.update_project_path(&temp_dir);
        state.workflow_stage = "Build".to_string();
        state.plan_approved = true;
        state.edit_authorized = true;
        state.is_fast_path = true;
        state.verification_passed = true;
        state.user_intent_summary = Some("Old task".to_string());
        state.queue_deferred_authorization(&["src/old.rs".to_string()]);
        state.pending_skill_proposals = vec![("old-skill".to_string(), "old-skill/SKILL.md".to_string(), 0.9)];

        handle(
            serde_json::json!({
                "task": "Implement brand new billing service",
                "project_path": temp_dir.to_str().unwrap(),
                "phase": "implement"
            }),
            &mut state,
        )
        .unwrap();

        assert_eq!(state.workflow_stage, "Plan");
        assert!(!state.plan_approved);
        assert!(!state.edit_authorized);
        assert!(!state.is_fast_path);
        assert!(!state.verification_passed);
        assert!(state.deferred_authorized_paths.is_empty());
        assert!(state.pending_skill_proposals.is_empty());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_pipeline_approval_msg_forces_build_preserves_task_and_clears_proposals() {
        let temp_dir = std::env::temp_dir().join(format!("ag_pipe_appr_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        let _ = std::fs::create_dir_all(&temp_dir);

        let mut state = ServerState::new();
        state.update_project_path(&temp_dir);
        state.workflow_stage = "Plan".to_string();
        state.user_intent_summary = Some("Build payment webhook".to_string());
        state.active_task = Some("Build payment webhook".to_string());
        state.pending_skill_proposals = vec![("stripe".to_string(), "stripe/SKILL.md".to_string(), 0.9)];

        let out = handle(
            serde_json::json!({
                "task": "Proceed",
                "project_path": temp_dir.to_str().unwrap(),
                "phase": "plan"
            }),
            &mut state,
        )
        .unwrap();

        assert_eq!(state.workflow_stage, "Build");
        assert!(state.plan_approved);
        assert!(!state.edit_authorized);
        assert!(state.pending_skill_proposals.is_empty());
        assert_eq!(state.active_task.as_deref(), Some("Build payment webhook"));
        assert_eq!(state.user_intent_summary.as_deref(), Some("Build payment webhook"));
        assert!(out.contains("Active Phase: build"));
        assert!(out.contains("Code Construction Mandates"));
        assert!(out.contains("authorize_edit"));
        assert!(!out.contains("Call `guidance(operation=\"search\""));

        let loaded = ServerState::load_from_dir(&temp_dir).unwrap();
        assert_eq!(loaded.workflow_stage, "Build");
        assert!(loaded.active_architecture_pattern.is_some());
        assert_eq!(loaded.active_task.as_deref(), Some("Build payment webhook"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_pipeline_rejects_out_of_workspace_project_path() {
        let base = std::env::temp_dir().join(format!("ag_pipe_oow_{}", std::process::id()));
        let repo_a = base.join("repo_a");
        let repo_b = base.join("repo_b");
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::create_dir_all(&repo_a);
        let _ = std::fs::create_dir_all(&repo_b);

        let mut state = ServerState::new();
        state.workspace_roots = vec![repo_a.to_string_lossy().to_string()];

        let err = handle(
            serde_json::json!({
                "task": "Inspect foreign repo",
                "project_path": repo_b.to_str().unwrap()
            }),
            &mut state,
        )
        .unwrap_err();
        assert_eq!(err.0, -32602);
        assert!(err.1.contains("outside active workspace roots"));

        let _ = std::fs::remove_dir_all(&base);
    }
}