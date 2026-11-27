use super::*;

#[test]
fn test_guidance_get_local_skill() {
    let mut state = ServerState::new();
    state.plan_approved = true;
    state.set_stage("Build").unwrap();

    let tmp_dir = std::env::temp_dir().join(format!("test_guidance_get_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp_dir);
    let skill_dir = tmp_dir.join(".agents").join("skills").join("custom");
    let _ = std::fs::create_dir_all(&skill_dir);
    let skill_file = skill_dir.join("SKILL.md");
    std::fs::write(
        &skill_file,
        "---\nname: custom\n---\n# Custom Skill Content",
    )
    .unwrap();

    let res = handle_tool_call(
        "guidance",
        json!({
            "operation": "get",
            "project_path": tmp_dir.to_str().unwrap(),
            "identifier": skill_file.to_string_lossy().to_string()
        }),
        &mut state,
    );

    assert!(res.is_ok());
    let text = res.unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(text.contains("Custom Skill Content"));

    let _ = std::fs::remove_dir_all(tmp_dir);
}

#[test]
fn test_anti_hallucination_verification() {
    let mut state = ServerState::new();
    state.plan_approved = true;
    state.set_stage("Test_Recheck").unwrap();

    let res = handle_tool_call("guidance", json!({ "operation": "verify" }), &mut state);

    assert!(res.is_ok());
    let text = res.unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(text.contains("Anti-Hallucination Post-Code Verification Checklist"));
    assert!(text.contains("User Requirement Alignment"));

    let check_res = handle_tool_call("workflow_gate", json!({ "action": "check" }), &mut state);
    assert!(check_res.is_ok());
    let check_text = check_res.unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(check_text.contains("ANTI-HALLUCINATION ENFORCER ACTIVE"));
}

#[test]
fn test_guidance_workflow_loads_embedded_reference() {
    let mut state = ServerState::new();
    let res = handle_tool_call(
        "guidance",
        json!({ "operation": "workflow", "identifier": "code" }),
        &mut state,
    );
    assert!(res.is_ok());
    let text = res.unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(text.contains("code") || text.contains("Code") || text.contains("Build"));
    assert!(!text.contains("# Dev Workflow Guidance: [code]\n\nRecommended Flow: Context -> Plan -> Ask/Revise -> Build -> Test/Recheck -> Fix -> Document"));
}

#[test]
fn test_guidance_precode_kotlin_and_go_rules() {
    let temp_dir = std::env::temp_dir().join(format!("precode_kotlin_test_{}", std::process::id()));
    let kt_dir = temp_dir.join("app").join("src").join("main").join("java").join("com").join("example");
    let _ = std::fs::create_dir_all(&kt_dir);
    let _ = std::fs::write(kt_dir.join("MainActivity.kt"), "class MainActivity");

    let mut state = ServerState::new();
    state.update_project_path(&temp_dir);
    let res = handle_tool_call(
        "guidance",
        json!({
            "operation": "precode",
            "query": "android kotlin UI",
            "project_path": temp_dir.to_str().unwrap()
        }),
        &mut state,
    );
    assert!(res.is_ok());
    let text = res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(text.contains("Primary Language: Kotlin/Java"));
    assert!(text.contains("Dispatchers.IO/Default"));
    assert!(text.contains("StateFlow/LiveData lifecycles"));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_precode_upfront_split_blueprint() {
    let mut state = ServerState::new();
    state.active_architecture_pattern = Some("CLI_Pipeline".to_string());

    let res = handle_tool_call(
        "guidance",
        json!({ "operation": "precode", "query": "rust" }),
        &mut state,
    );
    assert!(res.is_ok());
    let text = res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(text.contains("Domain Cohesion & Soft Quality Thresholds"));
    assert!(text.contains("CLI_Pipeline"));
    assert!(text.contains("cargo clippy"));
}

#[test]
fn test_guidance_reindex_skills_operation() {
    let temp_dir = std::env::temp_dir().join(format!("reindex_skills_test_{}", std::process::id()));
    let skill_dir = temp_dir.join(".agents").join("skills").join("my-custom-skill");
    let _ = std::fs::create_dir_all(&skill_dir);
    let skill_file = skill_dir.join("SKILL.md");
    std::fs::write(
        &skill_file,
        "---\nname: my-custom-skill\ndescription: A test custom skill\n---\n# My Custom Skill\n## When to Activate\n- Trigger when testing reindex\n",
    ).unwrap();

    let mut state = ServerState::new();
    let res = handle_tool_call(
        "guidance",
        json!({
            "operation": "reindex_skills",
            "project_path": temp_dir.to_str().unwrap()
        }),
        &mut state,
    );
    assert!(res.is_ok());
    let text = res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(text.contains("Skill Semantic Index Refreshed"));
    assert!(text.contains("Catalog Fingerprint:"));
    assert!(text.contains("reindexed with rich semantic passages"));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_guidance_search_ask_question_proposal_and_context_discovery() {
    let mut state = ServerState::new();
    let res = handle_tool_call(
        "guidance",
        json!({
            "operation": "search",
            "workflow": "android compose app architecture",
            "files": ["ui/screens/HomeScreen.kt"]
        }),
        &mut state,
    );
    assert!(res.is_ok());
    let text = res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(text.contains("SKILL_PROPOSAL: MANDATORY USER INTERACTION REQUIRED"));
    assert!(text.contains("ask_question"));
    assert!(text.contains("is_multi_select"));
    assert!(text.contains("user_confirmed=true"));
    assert!(!state.pending_skill_proposals.is_empty());
}

#[test]
fn test_task_pipeline_enriched_recommendations_formatting() {
    let temp_dir = std::env::temp_dir().join(format!("pipeline_enrich_test_{}", std::process::id()));
    let skill_dir = temp_dir.join(".agents").join("skills").join("sql-safety");
    let _ = std::fs::create_dir_all(&skill_dir);
    let skill_file = skill_dir.join("SKILL.md");
    std::fs::write(
        &skill_file,
        "---\nname: sql-safety\ndescription: SQL safety and injection defense rules for database queries.\n---\n# SQL Safety\n## Guidelines\n- Always use parameterized queries\n- Never concatenate raw strings\n",
    ).unwrap();

    let mut state = ServerState::new();
    let res = handle_tool_call(
        "task_pipeline",
        json!({
            "task": "Fix sql injection in database queries",
            "project_path": temp_dir.to_str().unwrap(),
            "phase": "implement"
        }),
        &mut state,
    );
    assert!(res.is_ok());
    let text = res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(text.contains("Task Pipeline Activated"));
    assert!(text.contains("Fix sql injection in database queries"));
    assert!(text.contains("Active Phase: implement"));
    assert!(text.contains("Architecture Guidance"));
    assert!(text.contains("Priority Gate: PASSED"));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_guidance_search_informational_question_bypasses_proposal() {
    let mut state = ServerState::new();
    let res = handle_tool_call(
        "guidance",
        json!({
            "operation": "search",
            "query": "Làm thế nào để index skill vào bin sau khi edit trên HTML?"
        }),
        &mut state,
    );
    assert!(res.is_ok());
    let text = res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(text.contains("Informational Query Detected"));
    assert!(text.contains("Bypassing skill proposals"));
    assert!(!text.contains("SKILL_PROPOSAL: MANDATORY USER INTERACTION REQUIRED"));
    assert!(state.pending_skill_proposals.is_empty());
}

#[test]
fn test_guidance_search_slash_command_alone_bypasses_proposal() {
    let mut state = ServerState::new();
    let res = handle_tool_call(
        "guidance",
        json!({
            "operation": "search",
            "query": "/generative_ui"
        }),
        &mut state,
    );
    assert!(res.is_ok());
    let text = res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(text.contains("Slash Command Detected: /generative_ui"));
    assert!(text.contains("Bypassing skill proposals"));
    assert!(!text.contains("SKILL_PROPOSAL: MANDATORY USER INTERACTION REQUIRED"));
    assert!(state.pending_skill_proposals.is_empty());
}

#[test]
fn test_guidance_search_slash_command_strips_and_gen_ui_prohibits_questions() {
    let mut state = ServerState::new();
    let res = handle_tool_call(
        "guidance",
        json!({"operation": "search", "query": "/automation backup cron"}),
        &mut state,
    );
    let text = res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(text.contains("backup cron") && !text.contains("/automation' ["));

    let res_ui = handle_tool_call(
        "guidance",
        json!({"operation": "search", "query": "/generative_ui render cards"}),
        &mut state,
    );
    let text_ui = res_ui.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(text_ui.contains("Generative UI Active"));
    assert!(text_ui.contains("Calling `ask_question` is STRICTLY PROHIBITED"));
    assert!(!text_ui.contains("SKILL_PROPOSAL: MANDATORY USER INTERACTION REQUIRED"));
    assert!(state.pending_skill_proposals.is_empty());
}

#[test]
fn test_guidance_search_mentioned_skill_prioritized_with_recommended() {
    let mut state = ServerState::new();
    let res = handle_tool_call(
        "guidance",
        json!({
            "operation": "search",
            "query": "Dùng agent-guidance để kiểm tra quy chuẩn"
        }),
        &mut state,
    );
    assert!(res.is_ok());
    let text = res.unwrap()["content"][0]["text"].as_str().unwrap().to_string();
    assert!(text.contains("SKILL_PROPOSAL: MANDATORY USER INTERACTION REQUIRED"));
    assert!(text.contains("ask_question"));
    assert!(text.contains("(Recommended) agent-guidance"));
}

#[test]
fn test_guidance_get_and_workflow_accept_topic_param() {
    let mut state = ServerState::new();
    let get_res = handle_tool_call(
        "guidance",
        json!({ "operation": "get", "topic": "agent-guidance" }),
        &mut state,
    )
    .unwrap();
    let get_text = get_res["content"][0]["text"].as_str().unwrap();
    assert!(!get_text.contains("Skill asset not found"));

    let wf_res = handle_tool_call(
        "guidance",
        json!({ "operation": "workflow", "topic": "code" }),
        &mut state,
    )
    .unwrap();
    let wf_text = wf_res["content"][0]["text"].as_str().unwrap();
    assert!(!wf_text.contains("# Dev Workflow Guidance: [plan]"));
}

#[test]
fn test_guidance_search_empty_selection_and_word_boundary_ui_note() {
    let mut state = ServerState::new();
    let res = handle_tool_call(
        "guidance",
        json!({
            "operation": "search",
            "query": "android compose app architecture",
            "max_results": 2
        }),
        &mut state,
    )
    .unwrap();
    let text = res["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("select_skills(skills=[], user_confirmed=true)"));
    assert!(state.pending_skill_proposals.len() <= 2);

    // "build guide require suite" must NOT trigger Generative UI Protocol note
    let out = crate::mcp::tools::guidance_format::format_search_output(
        "build guide require suite",
        None,
        &["- rust-patterns [Embedded] (Score: 0.90)".to_string()],
        &["(Recommended) rust-patterns - Rust patterns".to_string()],
        "TestEngine",
    )
    .unwrap();
    assert!(!out.contains("Generative UI Protocol"));

    // Explicit "ui" word DOES trigger Generative UI Protocol note
    let out_ui = crate::mcp::tools::guidance_format::format_search_output(
        "build web ui",
        None,
        &["- frontend-design [Embedded] (Score: 0.90)".to_string()],
        &["(Recommended) frontend-design - UI design".to_string()],
        "TestEngine",
    )
    .unwrap();
    assert!(out_ui.contains("Generative UI Protocol"));
}

#[test]
fn test_guidance_intent_runs_before_files_and_preserves_order() {
    let mut state = ServerState::new();
    let genui = handle_tool_call(
        "guidance",
        json!({
            "operation": "search",
            "query": "/generative-ui",
            "files": ["src/App.tsx"],
            "tech_stack": "react"
        }),
        &mut state,
    )
    .unwrap();
    let genui_text = genui["content"][0]["text"].as_str().unwrap();
    assert!(genui_text.contains("Slash Command Detected: /generative_ui"));
    assert!(genui_text.contains("Calling `ask_question` is STRICTLY PROHIBITED"));

    // Case-insensitive deduplication of active_task in context_note
    state.active_task = Some("Implement Auth Middleware".to_string());
    state.push_recent_task("Implement Auth Middleware");
    let ctx_res = handle_tool_call(
        "guidance",
        json!({
            "operation": "search",
            "query": "implement auth middleware"
        }),
        &mut state,
    )
    .unwrap();
    let ctx_text = ctx_res["content"][0]["text"].as_str().unwrap();
    assert!(!ctx_text.contains("Active: Implement Auth Middleware"));
}

#[test]
fn test_guidance_unknown_op_workflow_traversal_list_filter_and_precode_files() {
    let mut state = ServerState::new();
    let err = handle_tool_call(
        "guidance",
        json!({ "operation": "does_not_exist" }),
        &mut state,
    )
    .unwrap_err();
    assert_eq!(err.0, -32602);
    assert!(err.1.contains("Invalid guidance operation"));
    assert!(err.1.contains("analytics"));

    let trav_err = handle_tool_call(
        "guidance",
        json!({ "operation": "workflow", "identifier": "../secret" }),
        &mut state,
    )
    .unwrap_err();
    assert_eq!(trav_err.0, -32602);

    let list_res = handle_tool_call(
        "guidance",
        json!({ "operation": "list", "topic": "rust", "limit": 2 }),
        &mut state,
    )
    .unwrap();
    let list_text = list_res["content"][0]["text"].as_str().unwrap();
    assert!(list_text.contains("Registered Skills Catalog"));
    assert!(list_text.lines().filter(|l| l.starts_with("- ")).count() <= 2);

    // Precode uses target files/framework instead of repo-wide Rust precedence
    let precode_ts = handle_tool_call(
        "guidance",
        json!({
            "operation": "precode",
            "files": ["src/components/Dashboard.tsx"],
            "framework": "React",
            "project_path": "."
        }),
        &mut state,
    )
    .unwrap();
    let precode_text = precode_ts["content"][0]["text"].as_str().unwrap();
    assert!(precode_text.contains("Primary Language: TypeScript/JavaScript"));
    assert!(precode_text.contains("eslint"));

    // "plan" must NOT match "explanation"
    assert_eq!(
        crate::mcp::tools::pipeline_fast_path::infer_or_normalize_phase(Some("plan"), "Fix the explanation bug"),
        "fix"
    );

    // Order of multiple mentioned skills preserved in tier_skill_results
    let items = vec![
        (0.95f32, crate::catalog::store::SkillItem {
            name: "alpha-skill".to_string(),
            relative_path: "alpha-skill/SKILL.md".to_string(),
            source: crate::catalog::store::SkillSource::Embedded,
            content: "# Alpha".to_string(),
        }),
        (0.95f32, crate::catalog::store::SkillItem {
            name: "beta-skill".to_string(),
            relative_path: "beta-skill/SKILL.md".to_string(),
            source: crate::catalog::store::SkillSource::Embedded,
            content: "# Beta".to_string(),
        }),
    ];
    let tiered = crate::mcp::tools::guidance_search_stages::tier_skill_results(
        items,
        &["beta-skill".to_string(), "alpha-skill".to_string()],
        6,
    );
    assert_eq!(tiered[0].1.name, "beta-skill");
    assert_eq!(tiered[1].1.name, "alpha-skill");
}

#[test]
fn test_task_pipeline_compacted_output_omits_json_template_and_duplicate_rules() {
    let mut state = ServerState::new();
    let res = handle_tool_call(
        "task_pipeline",
        json!({
            "task": "Refactor auth module and compact mcp output",
            "phase": "plan"
        }),
        &mut state,
    )
    .unwrap();
    let text = res["content"][0]["text"].as_str().unwrap();
    assert!(!text.contains("ask_question({"));
    assert!(!text.contains("CRITICAL MULTILINGUAL MANDATE"));
    assert!(!text.contains("[Rule:task_pipeline"));
    let toks = crate::optimizer::compressor::estimate_tokens(text, false);
    assert!(toks < 650, "Expected compacted task_pipeline < 650 tokens, got {}", toks);
}
