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
fn test_non_linear_stack_reports_unsupported_merge() {
    let repo = TempJjRepo::new();
    for (bookmark, message) in [
        ("bookmark-b", "feature B"),
        ("bookmark-c", "feature C"),
        ("bookmark-d", "feature D"),
    ] {
        repo.run_jj(&["new", "root()", "-m", message]);
        repo.create_bookmark(bookmark);
    }
    repo.run_jj(&[
        "new",
        "bookmark-b",
        "bookmark-c",
        "bookmark-d",
        "-m",
        "merge",
    ]);

    let mut cmd = Command::cargo_bin("ryu").unwrap();
    cmd.current_dir(repo.path());

    cmd.assert()
        .failure()
        .stderr(predicate::str::contains(
            "unsupported non-linear stack: merge commit",
        ))
        .stderr(predicate::str::contains("rebase into a linear stack"))
        .stderr(predicate::str::contains("No bookmark stack found").not());
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

// =============================================================================
// Git Integration Tests
// =============================================================================

fn bare_remote_bookmark_oid(remote_path: &std::path::Path, bookmark: &str) -> String {
    let output = StdCommand::new("git")
        .args(["rev-parse", &format!("refs/heads/{bookmark}")])
        .current_dir(remote_path)
        .output()
        .expect("read bare remote bookmark");
    assert!(
        output.status.success(),
        "remote bookmark should exist: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn remote_bookmark_state(
    repo_path: &std::path::Path,
    bookmark: &str,
    remote: &str,
) -> jj_lib::op_store::RemoteRefState {
    use jj_lib::config::{ConfigLayer, ConfigSource, StackedConfig};
    use jj_lib::default_backend_factories::{
        default_backend_factories, default_working_copy_factories,
    };
    use jj_lib::ref_name::{RefName, RemoteName};
    use jj_lib::settings::UserSettings;
    use jj_lib::workspace::Workspace;

    let mut config = StackedConfig::with_defaults();
    let mut user_layer = ConfigLayer::empty(ConfigSource::User);
    user_layer
        .set_value("user.name", "jj-ryu-test")
        .expect("set test user name");
    user_layer
        .set_value("user.email", "jj-ryu-test@localhost")
        .expect("set test user email");
    config.add_layer(user_layer);
    let settings = UserSettings::from_config(config).expect("create test settings");
    let workspace = Workspace::load(
        &settings,
        repo_path,
        &default_backend_factories(),
        &default_working_copy_factories(),
    )
    .expect("load jj workspace");
    let repo = pollster::block_on(workspace.repo_loader().load_at_head()).expect("load jj repo");
    let symbol = RefName::new(bookmark).to_remote_symbol(RemoteName::new(remote));
    repo.view().get_remote_bookmark(symbol).state
}

#[test]
fn test_git_push_updates_remote_and_tracking_bookmark() {
    let repo = TempJjRepo::new();
    repo.commit("Add feature A");
    repo.create_bookmark("feat-a");
    let (_remote_dir, remote_path) = TempJjRepo::create_bare_remote();
    repo.add_remote("origin", &remote_path);

    let mut workspace = repo.workspace();
    workspace
        .git_push("feat-a", "origin")
        .expect("push feat-a bookmark");

    let remote_oid = bare_remote_bookmark_oid(&remote_path, "feat-a");
    let bookmark = workspace
        .get_local_bookmark("feat-a")
        .expect("read local bookmark")
        .expect("feat-a bookmark should exist");
    assert_eq!(remote_oid, bookmark.commit_id);
    assert_eq!(
        remote_bookmark_state(repo.path(), "feat-a", "origin"),
        jj_lib::op_store::RemoteRefState::Tracked
    );
    assert!(bookmark.has_remote);
    assert!(bookmark.is_synced);
}

#[test]
fn test_git_push_rejects_stale_remote_lease_without_updating_tracking() {
    let repo = TempJjRepo::new();
    repo.commit("Add feature A");
    repo.create_bookmark("feat-a");
    let (_remote_dir, remote_path) = TempJjRepo::create_bare_remote();
    repo.add_remote("origin", &remote_path);

    let mut workspace = repo.workspace();
    workspace
        .git_push("feat-a", "origin")
        .expect("initial push should succeed");
    let original_remote_oid = bare_remote_bookmark_oid(&remote_path, "feat-a");

    let tree_oid = StdCommand::new("git")
        .args(["rev-parse", "refs/heads/feat-a^{tree}"])
        .current_dir(&remote_path)
        .output()
        .expect("read remote tree");
    assert!(tree_oid.status.success(), "read remote tree");
    let tree_oid = String::from_utf8_lossy(&tree_oid.stdout).trim().to_string();
    let remote_commit = StdCommand::new("git")
        .args([
            "-c",
            "user.name=Remote User",
            "-c",
            "user.email=remote@example.com",
            "commit-tree",
            &tree_oid,
            "-p",
            &original_remote_oid,
            "-m",
            "Advance remote",
        ])
        .current_dir(&remote_path)
        .output()
        .expect("create remote commit");
    assert!(
        remote_commit.status.success(),
        "create remote commit: {}",
        String::from_utf8_lossy(&remote_commit.stderr)
    );
    let advanced_remote_oid = String::from_utf8_lossy(&remote_commit.stdout)
        .trim()
        .to_string();
    let update_remote = StdCommand::new("git")
        .args([
            "update-ref",
            "refs/heads/feat-a",
            &advanced_remote_oid,
            &original_remote_oid,
        ])
        .current_dir(&remote_path)
        .output()
        .expect("advance remote bookmark");
    assert!(
        update_remote.status.success(),
        "advance remote bookmark: {}",
        String::from_utf8_lossy(&update_remote.stderr)
    );

    repo.commit("Advance local");
    repo.move_bookmark("feat-a", "@-");
    let mut workspace = repo.workspace();
    let local_commit_id = workspace
        .get_local_bookmark("feat-a")
        .expect("read local bookmark")
        .expect("feat-a bookmark should exist")
        .commit_id;

    let error = workspace
        .git_push("feat-a", "origin")
        .expect_err("stale remote lease should reject push");
    let error_message = error.to_string();
    assert!(error_message.contains("lease"), "{error_message}");
    assert!(
        error_message.contains("refs/heads/feat-a"),
        "{error_message}"
    );
    assert_eq!(
        bare_remote_bookmark_oid(&remote_path, "feat-a"),
        advanced_remote_oid
    );

    let remote_bookmark = workspace
        .get_remote_bookmark("feat-a", "origin")
        .expect("read tracked remote bookmark")
        .expect("tracked remote bookmark should exist");
    assert_eq!(remote_bookmark.commit_id, original_remote_oid);
    assert_ne!(remote_bookmark.commit_id, local_commit_id);
    assert_eq!(
        remote_bookmark_state(repo.path(), "feat-a", "origin"),
        jj_lib::op_store::RemoteRefState::Tracked
    );
}

/// Test that `git_fetch` handles rewrites after fetching rebased commits.
///
/// This reproduces issue #8 where `ryu sync` panicked with:
/// "BUG: Descendants have not been rebased after the last rewrites"
///
/// The scenario:
/// 1. User creates a bookmark and pushes to remote
/// 2. Remote rebases the commit (e.g., GitHub rebase merge)
/// 3. User fetches - jj detects the rewrite
/// 4. Without calling `rebase_descendants()`, `tx.commit()` panics
#[test]
fn test_git_fetch_handles_rebased_commits() {
    // Create a bare git repo to act as "remote"
    let (_remote_dir, remote_path) = TempJjRepo::create_bare_remote();

    // Create local jj repo
    let repo = TempJjRepo::new();

    // Add the bare repo as a remote
    repo.add_remote("origin", &remote_path);

    // Create a bookmark and push it
    // We need to create the bookmark on the commit with the description, not the working copy
    // jj commit -m "X" creates commit X and leaves WC as empty child
    // So we commit, then move bookmark to parent (@-)
    repo.commit("Add feature A");
    // Create bookmark on the parent (the actual commit with description)
    StdCommand::new("jj")
        .args(["bookmark", "create", "feat-a", "-r", "@-"])
        .current_dir(repo.path())
        .output()
        .expect("create bookmark");
    repo.push_bookmark("feat-a", "origin");

    // Simulate a rebase on the remote by creating a new commit with same content
    // but different commit ID (like GitHub rebase merge does)
    //
    // We do this by:
    // 1. Clone the bare repo to a temp location
    // 2. Checkout the branch
    // 3. Amend the commit to change its ID
    // 4. Force push to the bare repo
    let temp_clone = TempDir::new().expect("create temp clone dir");
    let clone_output = StdCommand::new("git")
        .args(["clone", &remote_path.to_string_lossy(), "."])
        .current_dir(temp_clone.path())
        .output()
        .expect("git clone failed");

    assert!(
        clone_output.status.success(),
        "git clone failed: {}",
        String::from_utf8_lossy(&clone_output.stderr)
    );

    // Checkout the feat-a branch
    let checkout_output = StdCommand::new("git")
        .args(["checkout", "feat-a"])
        .current_dir(temp_clone.path())
        .output()
        .expect("git checkout failed");

    assert!(
        checkout_output.status.success(),
        "git checkout failed: {}",
        String::from_utf8_lossy(&checkout_output.stderr)
    );

    // Configure git user for the clone
    StdCommand::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(temp_clone.path())
        .output()
        .expect("git config email");
    StdCommand::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(temp_clone.path())
        .output()
        .expect("git config name");

    // Create a new commit with --amend to change the commit ID
    // This simulates what happens when GitHub does a rebase merge
    let amend_output = StdCommand::new("git")
        .args([
            "commit",
            "--amend",
            "--allow-empty",
            "-m",
            "Add feature A (rebased)",
            "--date",
            "2026-01-01T00:00:00",
        ])
        .current_dir(temp_clone.path())
        .output()
        .expect("git amend failed");

    assert!(
        amend_output.status.success(),
        "git commit --amend failed: {}",
        String::from_utf8_lossy(&amend_output.stderr)
    );

    // Force push the amended commit to the bare repo
    let push_output = StdCommand::new("git")
        .args(["push", "--force", "origin", "feat-a"])
        .current_dir(temp_clone.path())
        .output()
        .expect("git push failed");

    assert!(
        push_output.status.success(),
        "git push --force failed: {}",
        String::from_utf8_lossy(&push_output.stderr)
    );

    // Now fetch from the remote - this should NOT panic
    // Before the fix, this would panic with:
    // "BUG: Descendants have not been rebased after the last rewrites"
    let mut workspace = repo.workspace();
    let result = workspace.git_fetch("origin");

    assert!(
        result.is_ok(),
        "git_fetch should succeed after remote rebase, got: {:?}",
        result.err()
    );
}

use std::process::Command as StdCommand;
use tempfile::TempDir;
