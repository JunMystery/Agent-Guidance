use std::fs;
use std::path::{Path, PathBuf};
use crate::dashboard::projects_container::is_generic_container_dir;
use crate::dashboard::projects_path::find_project_root;
use crate::mcp::state::ServerState;
use crate::mcp::tools::detect_project_path;

fn temp_boundary_dir(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "ag_test_boundary_{}_{}_{}",
        name,
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_micros()
    ));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn test_generic_container_dir_detection() {
    assert!(is_generic_container_dir(Path::new("/home/user/Git")));
    assert!(is_generic_container_dir(Path::new("/home/user/Projects")));
    assert!(is_generic_container_dir(Path::new("C:\\Users\\dev\\workspace")));
    assert!(is_generic_container_dir(Path::new("/var/repos")));
    assert!(!is_generic_container_dir(Path::new("/home/user/Git/my-repo")));
}

#[test]
fn test_multi_git_repo_container_heuristic() {
    let container = temp_boundary_dir("multi_repo_container");
    let repo_a = container.join("repo_a");
    let repo_b = container.join("repo_b");
    fs::create_dir_all(repo_a.join(".git")).unwrap();
    fs::create_dir_all(repo_b.join(".git")).unwrap();

    assert!(is_generic_container_dir(&container));

    let _ = fs::remove_dir_all(&container);
}

#[test]
fn test_prevent_climbing_to_parent_container_even_with_agent_context() {
    let container = temp_boundary_dir("container_git").join("Git");
    fs::create_dir_all(&container).unwrap();
    fs::create_dir_all(container.join(".agent-context")).unwrap();

    let empty_proj = container.join("virdi-gateway");
    fs::create_dir_all(&empty_proj).unwrap();

    let resolved = find_project_root(&empty_proj);
    assert_eq!(resolved, empty_proj, "Must NOT climb to container even if container has .agent-context");

    let _ = fs::remove_dir_all(&container.parent().unwrap());
}

#[test]
fn test_respect_explicit_empty_project_dir_in_detect_path() {
    let container = temp_boundary_dir("container_git2").join("Git");
    fs::create_dir_all(&container).unwrap();
    fs::create_dir_all(container.join(".agent-context")).unwrap();

    let empty_proj = container.join("new-empty-service");
    fs::create_dir_all(&empty_proj).unwrap();

    let state = ServerState::new();
    let detected = detect_project_path(&empty_proj.to_string_lossy(), &state);
    assert_eq!(detected, empty_proj, "detect_project_path must preserve explicit empty project");

    let _ = fs::remove_dir_all(&container.parent().unwrap());
}

#[test]
fn test_legitimate_submodule_finds_enclosing_git_root() {
    let container = temp_boundary_dir("container_git3").join("Git");
    let real_repo = container.join("real-repo");
    let subfolder = real_repo.join("src").join("controllers");
    fs::create_dir_all(&subfolder).unwrap();
    fs::create_dir_all(real_repo.join(".git")).unwrap();

    let resolved = find_project_root(&subfolder);
    assert_eq!(resolved, real_repo, "Subfolder inside git repo must find repo root");

    let _ = fs::remove_dir_all(&container.parent().unwrap());
}
