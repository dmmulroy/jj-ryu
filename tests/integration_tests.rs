//! Integration tests for jj-ryu

#![allow(deprecated)] // cargo_bin is the standard way to test CLI binaries

mod common;

use assert_cmd::Command;
use common::{MockPlatformService, TempJjRepo, github_config, make_pr};
use jj_ryu::graph::build_change_graph;
use jj_ryu::submit::{ExecutionStep, analyze_submission, create_submission_plan};
use predicates::prelude::*;

// =============================================================================
// CLI Tests
// =============================================================================

#[test]
fn test_cli_help() {
    let mut cmd = Command::cargo_bin("ryu").unwrap();
    cmd.arg("--help");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Stacked PRs for Jujutsu"));
}

#[test]
fn test_cli_version() {
    let mut cmd = Command::cargo_bin("ryu").unwrap();
    cmd.arg("--version");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn test_submit_help() {
    let mut cmd = Command::cargo_bin("ryu").unwrap();
    cmd.args(["submit", "--help"]);

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Submit current stack"));
}

#[test]
fn test_sync_help() {
    let mut cmd = Command::cargo_bin("ryu").unwrap();
    cmd.args(["sync", "--help"]);

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Sync current stack"));
}

#[test]
fn test_auth_help() {
    let mut cmd = Command::cargo_bin("ryu").unwrap();
    cmd.args(["auth", "--help"]);

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("github"))
        .stdout(predicate::str::contains("gitlab"));
}

#[test]
fn test_invalid_path() {
    let mut cmd = Command::cargo_bin("ryu").unwrap();
    cmd.args(["--path", "/nonexistent/path/to/repo"]);

    cmd.assert().failure();
}

// =============================================================================
// Submit Flow Tests
// =============================================================================

#[test]
fn test_temp_repo_graph_building() {
    let repo = TempJjRepo::new();
    repo.build_stack(&[("feat-a", "Add A"), ("feat-b", "Add B")]);

    let workspace = repo.workspace();
    let graph = build_change_graph(&workspace).expect("build graph");

    // Should have both bookmarks
    assert!(graph.bookmarks.contains_key("feat-a"));
    assert!(graph.bookmarks.contains_key("feat-b"));

    // Should have one stack with two segments
    let stack = graph.stack.as_ref().expect("test expects stack");
    assert_eq!(stack.segments.len(), 2);
}

#[test]
fn test_analyze_real_repo_stack() {
    let repo = TempJjRepo::new();
    repo.build_stack(&[
        ("feat-a", "Add A"),
        ("feat-b", "Add B"),
        ("feat-c", "Add C"),
    ]);

    let workspace = repo.workspace();
    let graph = build_change_graph(&workspace).expect("build graph");

    // Analyze middle of stack
    let analysis = analyze_submission(&graph, Some("feat-b")).expect("analyze");

    assert_eq!(analysis.target_bookmark, "feat-b");
    assert_eq!(analysis.segments.len(), 2);
    assert_eq!(analysis.segments[0].bookmark.name, "feat-a");
    assert_eq!(analysis.segments[1].bookmark.name, "feat-b");
}

#[tokio::test]
async fn test_full_submit_flow_new_stack() {
    let repo = TempJjRepo::new();
    repo.build_stack(&[("feat-a", "Add feature A"), ("feat-b", "Add feature B")]);

    let workspace = repo.workspace();
    let graph = build_change_graph(&workspace).expect("build graph");
    let analysis = analyze_submission(&graph, Some("feat-b")).expect("analyze");

    // Mock returns None for all find_existing_pr calls (default behavior)
    let mock = MockPlatformService::with_config(github_config());

    let plan = create_submission_plan(&analysis, &mock, "origin", "main")
        .await
        .expect("create plan");

    // Verify plan
    assert_eq!(plan.count_creates(), 2);
    assert_eq!(plan.count_pushes(), 2);
    assert_eq!(plan.count_updates(), 0);

    // Find CreatePr steps and verify base branches
    let creates: Vec<_> = plan
        .execution_steps
        .iter()
        .filter_map(|s| match s {
            ExecutionStep::CreatePr(c) => Some(c),
            _ => None,
        })
        .collect();

    assert_eq!(creates[0].base_branch, "main");
    assert_eq!(creates[1].base_branch, "feat-a");

    // Verify titles are not empty
    assert!(!creates[0].title.is_empty());
    assert!(!creates[1].title.is_empty());
}

#[tokio::test]
async fn test_submit_flow_partial_existing_prs() {
    let repo = TempJjRepo::new();
    repo.build_stack(&[("feat-a", "Add A"), ("feat-b", "Add B")]);

    let workspace = repo.workspace();
    let graph = build_change_graph(&workspace).expect("build graph");
    let analysis = analyze_submission(&graph, Some("feat-b")).expect("analyze");

    let mock = MockPlatformService::with_config(github_config());
    // First PR exists
    mock.set_find_pr_response("feat-a", Some(make_pr(1, "feat-a", "main")));
    // Second PR doesn't exist (default)

    let plan = create_submission_plan(&analysis, &mock, "origin", "main")
        .await
        .expect("create plan");

    // Only one PR to create
    assert_eq!(plan.count_creates(), 1);

    let create = plan
        .execution_steps
        .iter()
        .find_map(|s| match s {
            ExecutionStep::CreatePr(c) => Some(c),
            _ => None,
        })
        .expect("should have create step");

    assert_eq!(create.bookmark.name, "feat-b");

    // One existing PR
    assert_eq!(plan.existing_prs.len(), 1);
    assert!(plan.existing_prs.contains_key("feat-a"));
}

#[tokio::test]
async fn test_submit_flow_base_update_needed() {
    let repo = TempJjRepo::new();
    repo.build_stack(&[("feat-a", "Add A"), ("feat-b", "Add B")]);

    let workspace = repo.workspace();
    let graph = build_change_graph(&workspace).expect("build graph");
    let analysis = analyze_submission(&graph, Some("feat-b")).expect("analyze");

    let mock = MockPlatformService::with_config(github_config());
    // Both PRs exist
    mock.set_find_pr_response("feat-a", Some(make_pr(1, "feat-a", "main")));
    // Second PR has wrong base (should be feat-a, is main)
    mock.set_find_pr_response("feat-b", Some(make_pr(2, "feat-b", "main")));

    let plan = create_submission_plan(&analysis, &mock, "origin", "main")
        .await
        .expect("create plan");

    // No PRs to create
    assert_eq!(plan.count_creates(), 0);

    // One PR needs base update
    assert_eq!(plan.count_updates(), 1);

    let update = plan
        .execution_steps
        .iter()
        .find_map(|s| match s {
            ExecutionStep::UpdateBase(u) => Some(u),
            _ => None,
        })
        .expect("should have update step");

    assert_eq!(update.bookmark.name, "feat-b");
    assert_eq!(update.current_base, "main");
    assert_eq!(update.expected_base, "feat-a");
}

#[test]
fn test_single_bookmark_stack() {
    let repo = TempJjRepo::new();
    repo.build_stack(&[("feat-a", "Add feature A")]);

    let workspace = repo.workspace();
    let graph = build_change_graph(&workspace).expect("build graph");
    let analysis = analyze_submission(&graph, Some("feat-a")).expect("analyze");

    assert_eq!(analysis.segments.len(), 1);
    assert_eq!(analysis.segments[0].bookmark.name, "feat-a");
}

// =============================================================================
// Edge Case Tests
// =============================================================================

#[test]
fn test_empty_repo_no_bookmarks() {
    let repo = TempJjRepo::new();
    // Don't create any bookmarks, just use initial state

    let workspace = repo.workspace();
    let graph = build_change_graph(&workspace).expect("build graph");

    // Should have no stacks
    assert!(graph.stack.is_none());
    assert!(graph.bookmarks.is_empty());
}

#[test]
fn test_three_level_deep_stack() {
    let repo = TempJjRepo::new();
    repo.build_stack(&[
        ("feat-a", "Add feature A"),
        ("feat-b", "Add feature B"),
        ("feat-c", "Add feature C"),
    ]);

    let workspace = repo.workspace();
    let graph = build_change_graph(&workspace).expect("build graph");

    let stack = graph.stack.as_ref().expect("test expects stack");
    assert_eq!(stack.segments.len(), 3);

    // Verify ordering: root to leaf
    assert_eq!(stack.segments[0].bookmarks[0].name, "feat-a");
    assert_eq!(stack.segments[1].bookmarks[0].name, "feat-b");
    assert_eq!(stack.segments[2].bookmarks[0].name, "feat-c");
}

#[tokio::test]
async fn test_plan_verifies_pr_queries_for_stack() {
    let repo = TempJjRepo::new();
    repo.build_stack(&[
        ("feat-a", "Add A"),
        ("feat-b", "Add B"),
        ("feat-c", "Add C"),
    ]);

    let workspace = repo.workspace();
    let graph = build_change_graph(&workspace).expect("build graph");
    let analysis = analyze_submission(&graph, Some("feat-c")).expect("analyze");

    let mock = MockPlatformService::with_config(github_config());

    let _ = create_submission_plan(&analysis, &mock, "origin", "main")
        .await
        .expect("create plan");

    // Verify that platform was queried for each bookmark in the stack
    mock.assert_find_pr_called_for(&["feat-a", "feat-b", "feat-c"]);
}

#[tokio::test]
async fn test_plan_pr_numbers_increment() {
    let repo = TempJjRepo::new();
    repo.build_stack(&[("feat-a", "Add A"), ("feat-b", "Add B")]);

    let workspace = repo.workspace();
    let graph = build_change_graph(&workspace).expect("build graph");
    let analysis = analyze_submission(&graph, Some("feat-b")).expect("analyze");

    let mock = MockPlatformService::with_config(github_config());

    let plan = create_submission_plan(&analysis, &mock, "origin", "main")
        .await
        .expect("create plan");

    // Verify we have 2 PRs to create
    assert_eq!(plan.count_creates(), 2);

    // Note: PR creation happens during execute, not planning
    // This test verifies the plan structure is correct
    let creates: Vec<_> = plan
        .execution_steps
        .iter()
        .filter_map(|s| match s {
            ExecutionStep::CreatePr(c) => Some(c),
            _ => None,
        })
        .collect();

    assert_eq!(creates[0].bookmark.name, "feat-a");
    assert_eq!(creates[1].bookmark.name, "feat-b");
}
