use super::schema::*;
use super::store::apply_env_overrides;

#[test]
fn test_default_config() {
    let cfg = AppConfig::default();
    assert_eq!(cfg.server.mode, "local");
    assert_eq!(cfg.server.url, DEFAULT_SERVER_URL);
    assert_eq!(cfg.server.timeout_ms, 3500);
    assert!(!cfg.server.is_remote());
    assert!(cfg.resilience.fallback_to_local);
    assert_eq!(cfg.dashboard.port, DEFAULT_DASHBOARD_PORT);
}

#[test]
fn test_endpoint_url_formatting() {
    let mut server = ServerConfig::default();
    server.url = "http://172.16.11.24:9000/".to_string();
    assert_eq!(
        server.endpoint_url("/api/skills/search"),
        "http://172.16.11.24:9000/api/skills/search"
    );
    assert_eq!(
        server.endpoint_url("health"),
        "http://172.16.11.24:9000/health"
    );
}

#[test]
fn test_toml_roundtrip() {
    let mut cfg = AppConfig::default();
    cfg.server.mode = "remote".to_string();
    cfg.server.url = "http://10.0.0.5:11998".to_string();
    cfg.server.api_key = "secret-token".to_string();
    cfg.server.timeout_ms = 5000;
    cfg.dashboard.port = 8080;

    let serialized = toml::to_string(&cfg).expect("Serialize toml");
    let deserialized: AppConfig = toml::from_str(&serialized).expect("Deserialize toml");

    assert_eq!(cfg, deserialized);
    assert!(deserialized.server.is_remote());
}

#[test]
fn test_env_overrides() {
    unsafe {
        std::env::set_var("AGENT_GUIDANCE_SERVER_MODE", "remote");
        std::env::set_var("AGENT_GUIDANCE_SERVER_URL", "http://192.168.1.100:9999");
        std::env::set_var("AGENT_GUIDANCE_API_KEY", "env-key-123");
        std::env::set_var("AGENT_GUIDANCE_TIMEOUT_MS", "4200");
        std::env::set_var("AGENT_GUIDANCE_DASHBOARD_PORT", "12345");
        std::env::set_var("AGENT_GUIDANCE_BIND_ADDR", "0.0.0.0");
    }

    let mut cfg = AppConfig::default();
    apply_env_overrides(&mut cfg);

    assert_eq!(cfg.server.mode, "remote");
    assert_eq!(cfg.server.url, "http://192.168.1.100:9999");
    assert_eq!(cfg.server.api_key, "env-key-123");
    assert_eq!(cfg.server.timeout_ms, 4200);
    assert_eq!(cfg.dashboard.port, 12345);
    assert_eq!(cfg.dashboard.bind, "0.0.0.0");

    // Cleanup env vars
    unsafe {
        std::env::remove_var("AGENT_GUIDANCE_SERVER_MODE");
        std::env::remove_var("AGENT_GUIDANCE_SERVER_URL");
        std::env::remove_var("AGENT_GUIDANCE_API_KEY");
        std::env::remove_var("AGENT_GUIDANCE_TIMEOUT_MS");
        std::env::remove_var("AGENT_GUIDANCE_DASHBOARD_PORT");
        std::env::remove_var("AGENT_GUIDANCE_BIND_ADDR");
    }
}

#[test]
fn test_tool_rules_defaults() {
    let defaults = ToolRulesConfig::default_rules();
    assert_eq!(defaults.len(), 6);
    assert!(defaults.contains_key("task_pipeline"));
    assert!(defaults.contains_key("select_skills"));
    assert!(defaults.contains_key("guidance"));
    assert!(defaults.contains_key("project_context"));
    assert!(defaults.contains_key("workflow_gate"));
    assert!(defaults.contains_key("session_continuity"));

    for (name, item) in &defaults {
        assert!(item.enabled, "Tool {} should be enabled by default", name);
        assert!(!item.rule.trim().is_empty(), "Tool {} rule should not be empty", name);
    }
}

#[test]
fn test_project_rules_save_and_load() {
    let temp_dir = std::env::temp_dir().join(format!("ag_test_rules_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    let _ = std::fs::create_dir_all(&temp_dir);

    assert_eq!(super::project_rules::load_project_rules(&temp_dir), None);

    let mut rules = std::collections::HashMap::new();
    rules.insert(
        "task_pipeline".to_string(),
        ToolRuleItem::new(true, "Custom project rule for testing"),
    );
    let cfg = ToolRulesConfig { rules };

    let save_res = super::project_rules::save_project_rules(&temp_dir, &cfg);
    assert!(save_res.is_ok(), "save_project_rules failed: {:?}", save_res);

    let loaded = super::project_rules::load_project_rules(&temp_dir);
    assert!(loaded.is_some());
    let loaded_cfg = loaded.unwrap();
    assert_eq!(
        loaded_cfg.get_rule("task_pipeline").map(|i| i.rule.as_str()),
        Some("Custom project rule for testing")
    );

    let del_res = super::project_rules::delete_project_rules(&temp_dir);
    assert!(del_res.unwrap());
    assert_eq!(super::project_rules::load_project_rules(&temp_dir), None);

    let _ = std::fs::remove_dir_all(&temp_dir);
}
