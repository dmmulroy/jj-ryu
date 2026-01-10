//! End-to-end tests with real GitHub API
//!
//! These tests require:
//! - `JJ_RYU_E2E_TESTS=1` environment variable
//! - `gh` CLI authenticated with repo scope
//! - `jj` CLI installed
//! - Test repo: `dmmulroy/jj-ryu-test`
//!
//! Run with: `JJ_RYU_E2E_TESTS=1 cargo test --test e2e_tests -- --include-ignored`

use jj_ryu::platform::{GitHubService, PlatformService};
use jj_ryu::submit::STACK_COMMENT_THIS_PR;
use jj_ryu::types::Platform;
use std::env;
use std::path::PathBuf;
use std::process::{Command, Output};
use tempfile::TempDir;
use uuid::Uuid;

const TEST_OWNER: &str = "dmmulroy";
const TEST_REPO: &str = "jj-ryu-test";

/// Check if E2E tests should run
fn e2e_enabled() -> bool {
    env::var("JJ_RYU_E2E_TESTS").is_ok()
}

/// Get GitHub token from gh CLI
fn get_gh_token() -> Option<String> {
    let output = Command::new("gh").args(["auth", "token"]).output().ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

/// Generate unique prefix for this test run
fn unique_prefix() -> String {
    let id = Uuid::new_v4().to_string()[..8].to_string();
    format!("e2e-{id}")
}

/// Generate unique branch name
fn unique_branch(prefix: &str) -> String {
    let id = Uuid::new_v4().to_string()[..8].to_string();
    format!("e2e-{prefix}-{id}")
}

fn repo_spec() -> String {
    format!("{TEST_OWNER}/{TEST_REPO}")
}

// =============================================================================
// Test Context (for API-level tests)
// =============================================================================

struct TestContext {
    service: GitHubService,
    created_branches: Vec<String>,
    created_prs: Vec<u64>,
}

impl TestContext {
    fn new() -> Option<Self> {
        if !e2e_enabled() {
            return None;
        }

        let token = get_gh_token()?;
        let service = GitHubService::new(&token, TEST_OWNER.into(), TEST_REPO.into(), None).ok()?;

        Some(Self {
            service,
            created_branches: vec![],
            created_prs: vec![],
        })
    }

    fn track_branch(&mut self, branch: &str) {
        self.created_branches.push(branch.to_string());
    }

    fn track_pr(&mut self, pr_number: u64) {
        self.created_prs.push(pr_number);
    }

    /// Push a branch with a test file via GitHub API
    #[allow(clippy::unused_self)] // kept as method for consistency
    fn push_branch(&self, branch: &str, content: &str) -> bool {
        Self::push_branch_impl(branch, "main", content)
    }

    /// Push a branch based on another branch
    #[allow(clippy::unused_self)] // kept as method for consistency
    fn push_branch_on_base(&self, branch: &str, base_branch: &str, content: &str) -> bool {
        Self::push_branch_impl(branch, base_branch, content)
    }

    /// Core implementation for pushing a branch
    fn push_branch_impl(branch: &str, base_ref: &str, content: &str) -> bool {
        let repo_spec = repo_spec();

        let base_sha = gh_api_get(
            &format!("repos/{repo_spec}/git/ref/heads/{base_ref}"),
            ".object.sha",
        );
        let Some(base_sha) = base_sha else {
            return false;
        };

        let blob_sha = gh_api_post(
            &format!("repos/{repo_spec}/git/blobs"),
            &[
                ("-f", format!("content={content}")),
                ("-f", "encoding=utf-8".into()),
            ],
            ".sha",
        );
        let Some(blob_sha) = blob_sha else {
            return false;
        };

        let base_tree = gh_api_get(
            &format!("repos/{repo_spec}/git/commits/{base_sha}"),
            ".tree.sha",
        );
        let Some(base_tree) = base_tree else {
            return false;
        };

        let new_tree = gh_api_post(
            &format!("repos/{repo_spec}/git/trees"),
            &[
                ("-f", format!("base_tree={base_tree}")),
                ("-f", format!("tree[][path]={branch}.txt")),
                ("-f", "tree[][mode]=100644".into()),
                ("-f", "tree[][type]=blob".into()),
                ("-f", format!("tree[][sha]={blob_sha}")),
            ],
            ".sha",
        );
        let Some(new_tree) = new_tree else {
            return false;
        };

        let commit_sha = gh_api_post(
            &format!("repos/{repo_spec}/git/commits"),
            &[
                ("-f", format!("message=test: {branch}")),
                ("-f", format!("tree={new_tree}")),
                ("-f", format!("parents[]={base_sha}")),
            ],
            ".sha",
        );
        let Some(commit_sha) = commit_sha else {
            return false;
        };

        gh_api_post(
            &format!("repos/{repo_spec}/git/refs"),
            &[
                ("-f", format!("ref=refs/heads/{branch}")),
                ("-f", format!("sha={commit_sha}")),
            ],
            ".sha",
        )
        .is_some()
    }

    fn cleanup(&self) {
        cleanup_branches_and_prs(&self.created_branches, &self.created_prs);
    }
}

// =============================================================================
// E2E Repo (for CLI-level tests)
// =============================================================================

/// Real jj repo for testing CLI commands
struct E2ERepo {
    dir: TempDir,
    prefix: String,
    created_bookmarks: Vec<String>,
}

impl E2ERepo {
    /// Clone test repo and init jj
    fn new() -> Option<Self> {
        if !e2e_enabled() {
            return None;
        }

        let dir = TempDir::new().ok()?;
        let prefix = unique_prefix();

        // Clone via gh CLI (handles auth automatically)
        let clone = Command::new("gh")
            .args(["repo", "clone", &repo_spec(), dir.path().to_str()?])
            .output()
            .ok()?;

        if !clone.status.success() {
            eprintln!("Clone failed: {}", String::from_utf8_lossy(&clone.stderr));
            return None;
        }

        // Init jj colocated
        let jj_init = Command::new("jj")
            .args(["git", "init", "--colocate"])
            .current_dir(dir.path())
            .output()
            .ok()?;

        if !jj_init.status.success() {
            eprintln!(
                "jj init failed: {}",
                String::from_utf8_lossy(&jj_init.stderr)
            );
            return None;
        }

        Some(Self {
            dir,
            prefix,
            created_bookmarks: vec![],
        })
    }

    fn path(&self) -> &std::path::Path {
        self.dir.path()
    }

    /// Create a new commit with a file
    fn create_commit(&self, message: &str) -> bool {
        // Create new change
        let new_output = Command::new("jj")
            .args(["new", "-m", message])
            .current_dir(self.path())
            .output();

        if !new_output.map(|o| o.status.success()).unwrap_or(false) {
            return false;
        }

        // Create a file to have actual content
        let filename = format!("{}.txt", message.replace(' ', "-"));
        let file_path = self.path().join(&filename);
        if std::fs::write(&file_path, message).is_err() {
            return false;
        }

        // Squash into parent to finalize
        let squash = Command::new("jj")
            .args(["squash"])
            .current_dir(self.path())
            .output();

        squash.map(|o| o.status.success()).unwrap_or(false)
    }

    /// Create a bookmark at current commit
    fn create_bookmark(&mut self, name: &str) -> bool {
        let full_name = format!("{}-{name}", self.prefix);
        let output = Command::new("jj")
            .args(["bookmark", "create", &full_name])
            .current_dir(self.path())
            .output();

        if output.map(|o| o.status.success()).unwrap_or(false) {
            self.created_bookmarks.push(full_name);
            true
        } else {
            false
        }
    }

    /// Build a stack of commits with bookmarks
    /// Returns list of bookmark names created
    fn build_stack(&mut self, commits: &[(&str, &str)]) -> Vec<String> {
        let mut bookmarks = vec![];
        for (bookmark, message) in commits {
            assert!(
                self.create_commit(message),
                "Failed to create commit: {message}"
            );
            assert!(
                self.create_bookmark(bookmark),
                "Failed to create bookmark: {bookmark}"
            );
            bookmarks.push(format!("{}-{bookmark}", self.prefix));
        }
        bookmarks
    }

    /// Get the ryu binary path
    fn ryu_bin() -> PathBuf {
        let mut path = env::current_exe().unwrap();
        path.pop(); // Remove test binary name
        path.pop(); // Remove deps
        path.push("ryu");
        path
    }

    /// Run ryu command
    fn run_ryu(&self, args: &[&str]) -> Output {
        Command::new(Self::ryu_bin())
            .args(args)
            .current_dir(self.path())
            .output()
            .expect("Failed to run ryu")
    }

    /// Run ryu submit
    fn submit(&self, bookmark: &str) -> Output {
        self.run_ryu(&["submit", bookmark])
    }

    /// Run ryu submit --dry-run
    fn submit_dry_run(&self, bookmark: &str) -> Output {
        self.run_ryu(&["submit", bookmark, "--dry-run"])
    }

    /// Run ryu sync
    fn sync(&self) -> Output {
        self.run_ryu(&["sync"])
    }

    /// Cleanup: close PRs and delete branches
    fn cleanup(&self) {
        // Find PRs for our bookmarks
        let mut prs = vec![];
        for bookmark in &self.created_bookmarks {
            if let Some(pr_num) = find_pr_number(bookmark) {
                prs.push(pr_num);
            }
        }
        cleanup_branches_and_prs(&self.created_bookmarks, &prs);
    }
}

impl Drop for E2ERepo {
    fn drop(&mut self) {
        // Best-effort cleanup on drop
        if !self.created_bookmarks.is_empty() {
            self.cleanup();
        }
    }
}

// =============================================================================
// GitHub API Helpers
// =============================================================================

fn gh_api_get(endpoint: &str, jq: &str) -> Option<String> {
    let output = Command::new("gh")
        .args(["api", endpoint, "--jq", jq])
        .output()
        .ok()?;

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

fn gh_api_post(endpoint: &str, fields: &[(&str, String)], jq: &str) -> Option<String> {
    let mut args = vec!["api", endpoint];
    for (flag, value) in fields {
        args.push(flag);
        args.push(value);
    }
    args.push("--jq");
    args.push(jq);

    let output = Command::new("gh").args(&args).output().ok()?;

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

fn find_pr_number(branch: &str) -> Option<u64> {
    let output = Command::new("gh")
        .args([
            "pr",
            "list",
            "-R",
            &repo_spec(),
            "--head",
            branch,
            "--json",
            "number",
            "--jq",
            ".[0].number",
        ])
        .output()
        .ok()?;

    if output.status.success() {
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        s.parse().ok()
    } else {
        None
    }
}

fn get_pr_base(pr_number: u64) -> Option<String> {
    gh_api_get(
        &format!("repos/{}/pulls/{pr_number}", repo_spec()),
        ".base.ref",
    )
}

fn get_pr_comments(pr_number: u64) -> Vec<String> {
    // Use JSON array output to handle multi-line comment bodies correctly
    let output = Command::new("gh")
        .args([
            "api",
            &format!("repos/{}/issues/{pr_number}/comments", repo_spec()),
            "--jq",
            "[.[].body]",
        ])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let json_str = String::from_utf8_lossy(&o.stdout);
            serde_json::from_str::<Vec<String>>(&json_str).unwrap_or_default()
        }
        _ => vec![],
    }
}

fn merge_pr(pr_number: u64) -> bool {
    let output = Command::new("gh")
        .args([
            "pr",
            "merge",
            &pr_number.to_string(),
            "-R",
            &repo_spec(),
            "--squash",
            "--delete-branch",
        ])
        .output();

    output.map(|o| o.status.success()).unwrap_or(false)
}

/// Get PR state (OPEN, MERGED, CLOSED)
fn get_pr_state(pr_number: u64) -> Option<String> {
    gh_api_get(
        &format!("repos/{}/pulls/{pr_number}", repo_spec()),
        ".state",
    )
}

/// Poll until PR reaches merged state or timeout
async fn wait_for_pr_merged(pr_number: u64, timeout: std::time::Duration) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if let Some(state) = get_pr_state(pr_number)
            && state == "closed"
            && let Some(merged) = gh_api_get(
                &format!("repos/{}/pulls/{pr_number}", repo_spec()),
                ".merged",
            )
            && merged == "true"
        {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    false
}

fn cleanup_branches_and_prs(branches: &[String], prs: &[u64]) {
    let repo_spec = repo_spec();

    // Close PRs
    for pr_num in prs {
        let _ = Command::new("gh")
            .args([
                "pr",
                "close",
                &pr_num.to_string(),
                "-R",
                &repo_spec,
                "--delete-branch",
            ])
            .output();
    }

    // Delete remaining branches
    for branch in branches {
        let _ = Command::new("gh")
            .args([
                "api",
                "-X",
                "DELETE",
                &format!("repos/{repo_spec}/git/refs/heads/{branch}"),
            ])
            .output();
    }
}

// =============================================================================
// Basic Connectivity Tests (API-level)
// =============================================================================

#[tokio::test]
async fn test_github_service_config() {
    let Some(ctx) = TestContext::new() else {
        eprintln!("Skipping: set JJ_RYU_E2E_TESTS=1");
        return;
    };

    let config = ctx.service.config();
    assert_eq!(config.platform, Platform::GitHub);
    assert_eq!(config.owner, TEST_OWNER);
    assert_eq!(config.repo, TEST_REPO);
}

#[tokio::test]
async fn test_find_nonexistent_pr() {
    let Some(ctx) = TestContext::new() else {
        eprintln!("Skipping: set JJ_RYU_E2E_TESTS=1");
        return;
    };

    let result = ctx
        .service
        .find_existing_pr("nonexistent-branch-xyz-12345")
        .await;

    assert!(result.is_ok(), "API call failed: {result:?}");
    assert!(result.unwrap().is_none());
}

// =============================================================================
// API-Level E2E Tests
// =============================================================================

#[tokio::test]
#[ignore = "E2E test requiring JJ_RYU_E2E_TESTS=1"]
async fn test_create_and_find_pr() {
    let Some(mut ctx) = TestContext::new() else {
        eprintln!("Skipping: set JJ_RYU_E2E_TESTS=1");
        return;
    };

    let branch = unique_branch("create");
    ctx.track_branch(&branch);

    assert!(ctx.push_branch(&branch, "test content"), "Failed to push");

    let pr = ctx
        .service
        .create_pr(&branch, "main", &format!("Test PR: {branch}"))
        .await
        .expect("Failed to create PR");

    ctx.track_pr(pr.number);

    assert!(pr.number > 0);
    assert_eq!(pr.head_ref, branch);
    assert_eq!(pr.base_ref, "main");

    let found = ctx
        .service
        .find_existing_pr(&branch)
        .await
        .expect("Failed to find PR");

    assert!(found.is_some());
    assert_eq!(found.unwrap().number, pr.number);

    ctx.cleanup();
}

#[tokio::test]
#[ignore = "E2E test requiring JJ_RYU_E2E_TESTS=1"]
async fn test_update_pr_base() {
    let Some(mut ctx) = TestContext::new() else {
        eprintln!("Skipping: set JJ_RYU_E2E_TESTS=1");
        return;
    };

    let branch1 = unique_branch("base");
    let branch2 = unique_branch("head");
    ctx.track_branch(&branch1);
    ctx.track_branch(&branch2);

    assert!(ctx.push_branch(&branch1, "base"));
    assert!(ctx.push_branch_on_base(&branch2, &branch1, "head"));

    let pr1 = ctx
        .service
        .create_pr(&branch1, "main", "PR1")
        .await
        .expect("create PR1");
    ctx.track_pr(pr1.number);

    let pr2 = ctx
        .service
        .create_pr(&branch2, &branch1, "PR2")
        .await
        .expect("create PR2");
    ctx.track_pr(pr2.number);

    assert_eq!(pr2.base_ref, branch1);

    let updated = ctx
        .service
        .update_pr_base(pr2.number, "main")
        .await
        .expect("update base");

    assert_eq!(updated.base_ref, "main");

    ctx.cleanup();
}

#[tokio::test]
#[ignore = "E2E test requiring JJ_RYU_E2E_TESTS=1"]
async fn test_pr_comments() {
    let Some(mut ctx) = TestContext::new() else {
        eprintln!("Skipping: set JJ_RYU_E2E_TESTS=1");
        return;
    };

    let branch = unique_branch("comments");
    ctx.track_branch(&branch);

    assert!(ctx.push_branch(&branch, "comment test"));

    let pr = ctx
        .service
        .create_pr(&branch, "main", "Comment test")
        .await
        .expect("create PR");
    ctx.track_pr(pr.number);

    ctx.service
        .create_pr_comment(pr.number, "E2E test comment")
        .await
        .expect("create comment");

    let comments = ctx
        .service
        .list_pr_comments(pr.number)
        .await
        .expect("list comments");

    assert!(!comments.is_empty());
    assert_eq!(comments[0].body, "E2E test comment");

    ctx.service
        .update_pr_comment(pr.number, comments[0].id, "Updated")
        .await
        .expect("update comment");

    let comments = ctx.service.list_pr_comments(pr.number).await.unwrap();
    assert_eq!(comments[0].body, "Updated");

    ctx.cleanup();
}

#[tokio::test]
#[ignore = "E2E test requiring JJ_RYU_E2E_TESTS=1"]
async fn test_pr_stack_rebase() {
    let Some(mut ctx) = TestContext::new() else {
        eprintln!("Skipping: set JJ_RYU_E2E_TESTS=1");
        return;
    };

    let branch_a = unique_branch("stack-a");
    let branch_b = unique_branch("stack-b");
    let branch_c = unique_branch("stack-c");
    ctx.track_branch(&branch_a);
    ctx.track_branch(&branch_b);
    ctx.track_branch(&branch_c);

    assert!(ctx.push_branch(&branch_a, "A"));
    assert!(ctx.push_branch_on_base(&branch_b, &branch_a, "B"));
    assert!(ctx.push_branch_on_base(&branch_c, &branch_b, "C"));

    let pr_a = ctx
        .service
        .create_pr(&branch_a, "main", "PR A")
        .await
        .expect("create A");
    ctx.track_pr(pr_a.number);

    let pr_b = ctx
        .service
        .create_pr(&branch_b, &branch_a, "PR B")
        .await
        .expect("create B");
    ctx.track_pr(pr_b.number);

    let pr_c = ctx
        .service
        .create_pr(&branch_c, &branch_b, "PR C")
        .await
        .expect("create C");
    ctx.track_pr(pr_c.number);

    assert_eq!(pr_b.base_ref, branch_a);
    assert_eq!(pr_c.base_ref, branch_b);

    let updated_b = ctx
        .service
        .update_pr_base(pr_b.number, "main")
        .await
        .expect("update B");
    assert_eq!(updated_b.base_ref, "main");

    ctx.cleanup();
}

// =============================================================================
// CLI-Level E2E Tests (Graphite-like workflows)
// =============================================================================

#[tokio::test]
#[ignore = "E2E test requiring JJ_RYU_E2E_TESTS=1"]
async fn test_submit_new_stack() {
    let Some(mut repo) = E2ERepo::new() else {
        eprintln!("Skipping: set JJ_RYU_E2E_TESTS=1");
        return;
    };

    // Create 2-commit stack
    let bookmarks = repo.build_stack(&[("feat-a", "Add feature A"), ("feat-b", "Add feature B")]);

    // Submit leaf
    let output = repo.submit(&bookmarks[1]);
    assert!(
        output.status.success(),
        "submit failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify PRs created
    let pr_a = find_pr_number(&bookmarks[0]);
    let pr_b = find_pr_number(&bookmarks[1]);

    assert!(pr_a.is_some(), "PR for feat-a not found");
    assert!(pr_b.is_some(), "PR for feat-b not found");

    // Verify bases
    assert_eq!(get_pr_base(pr_a.unwrap()), Some("main".into()));
    assert_eq!(get_pr_base(pr_b.unwrap()), Some(bookmarks[0].clone()));

    repo.cleanup();
}

#[tokio::test]
#[ignore = "E2E test requiring JJ_RYU_E2E_TESTS=1"]
async fn test_submit_partial_stack() {
    let Some(mut repo) = E2ERepo::new() else {
        eprintln!("Skipping: set JJ_RYU_E2E_TESTS=1");
        return;
    };

    // Create 3-commit stack
    let bookmarks = repo.build_stack(&[
        ("feat-a", "Add A"),
        ("feat-b", "Add B"),
        ("feat-c", "Add C"),
    ]);

    // Submit only up to feat-b (not leaf)
    let output = repo.submit(&bookmarks[1]);
    assert!(
        output.status.success(),
        "submit failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify only 2 PRs created
    assert!(find_pr_number(&bookmarks[0]).is_some(), "PR for a missing");
    assert!(find_pr_number(&bookmarks[1]).is_some(), "PR for b missing");
    assert!(
        find_pr_number(&bookmarks[2]).is_none(),
        "PR for c should not exist"
    );

    repo.cleanup();
}

#[tokio::test]
#[ignore = "E2E test requiring JJ_RYU_E2E_TESTS=1"]
async fn test_submit_idempotent() {
    let Some(mut repo) = E2ERepo::new() else {
        eprintln!("Skipping: set JJ_RYU_E2E_TESTS=1");
        return;
    };

    let bookmarks = repo.build_stack(&[("feat-x", "Add X")]);

    // First submit
    let output1 = repo.submit(&bookmarks[0]);
    assert!(
        output1.status.success(),
        "first submit failed: {}",
        String::from_utf8_lossy(&output1.stderr)
    );

    let pr_num = find_pr_number(&bookmarks[0]).expect("PR should exist");

    // Second submit (no changes)
    let output2 = repo.submit(&bookmarks[0]);
    assert!(
        output2.status.success(),
        "second submit failed: {}",
        String::from_utf8_lossy(&output2.stderr)
    );

    // Same PR number (not duplicated)
    let pr_num2 = find_pr_number(&bookmarks[0]).expect("PR should still exist");
    assert_eq!(pr_num, pr_num2);

    repo.cleanup();
}

#[tokio::test]
#[ignore = "E2E test requiring JJ_RYU_E2E_TESTS=1"]
async fn test_stack_comments() {
    let Some(mut repo) = E2ERepo::new() else {
        eprintln!("Skipping: set JJ_RYU_E2E_TESTS=1");
        return;
    };

    // Create 3-level stack
    let bookmarks = repo.build_stack(&[
        ("stack-1", "First"),
        ("stack-2", "Second"),
        ("stack-3", "Third"),
    ]);

    let output = repo.submit(&bookmarks[2]);
    assert!(
        output.status.success(),
        "submit failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Collect PR numbers for all bookmarks
    let pr_numbers: Vec<u64> = bookmarks
        .iter()
        .map(|b| find_pr_number(b).expect("PR should exist"))
        .collect();

    // Check stack comments on each PR
    for (i, _bookmark) in bookmarks.iter().enumerate() {
        let pr_num = pr_numbers[i];
        let comments = get_pr_comments(pr_num);

        // Must have stack comment with JJ-RYU marker
        let stack_comment = comments
            .iter()
            .find(|c| c.contains("<!--- JJ-RYU_STACK:"))
            .unwrap_or_else(|| panic!("PR #{pr_num} missing JJ-RYU stack comment"));

        // All PRs in stack should be referenced
        for &other_pr in &pr_numbers {
            assert!(
                stack_comment.contains(&format!("#{other_pr}")),
                "Stack comment on PR #{pr_num} missing reference to #{other_pr}"
            );
        }

        // Current PR must have marker
        assert!(
            stack_comment.contains(&format!("#{pr_num} {STACK_COMMENT_THIS_PR}")),
            "PR #{pr_num} missing {STACK_COMMENT_THIS_PR} marker for current position. Comment: {stack_comment}"
        );

        // Other PRs should NOT have marker
        for (j, &other_pr) in pr_numbers.iter().enumerate() {
            if j != i {
                assert!(
                    !stack_comment.contains(&format!("#{other_pr} {STACK_COMMENT_THIS_PR}")),
                    "PR #{other_pr} incorrectly has {STACK_COMMENT_THIS_PR} marker on PR #{pr_num}'s comment"
                );
            }
        }
    }

    repo.cleanup();
}

#[tokio::test]
#[ignore = "E2E test requiring JJ_RYU_E2E_TESTS=1"]
async fn test_deep_stack() {
    let Some(mut repo) = E2ERepo::new() else {
        eprintln!("Skipping: set JJ_RYU_E2E_TESTS=1");
        return;
    };

    // Create 5-level stack
    let bookmarks = repo.build_stack(&[
        ("deep-1", "Level 1"),
        ("deep-2", "Level 2"),
        ("deep-3", "Level 3"),
        ("deep-4", "Level 4"),
        ("deep-5", "Level 5"),
    ]);

    let output = repo.submit(&bookmarks[4]);
    assert!(
        output.status.success(),
        "5-level submit failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify all 5 PRs created with correct chaining
    let mut prev_bookmark = "main".to_string();
    for bookmark in &bookmarks {
        let pr_num =
            find_pr_number(bookmark).unwrap_or_else(|| panic!("PR for {bookmark} not found"));
        let base = get_pr_base(pr_num).expect("get base");
        assert_eq!(base, prev_bookmark, "Wrong base for {bookmark}");
        prev_bookmark = bookmark.clone();
    }

    repo.cleanup();
}

#[tokio::test]
#[ignore = "E2E test requiring JJ_RYU_E2E_TESTS=1"]
async fn test_sync_after_merge() {
    let Some(mut repo) = E2ERepo::new() else {
        eprintln!("Skipping: set JJ_RYU_E2E_TESTS=1");
        return;
    };

    // Create A -> B -> C stack
    let bookmarks = repo.build_stack(&[
        ("sync-a", "Sync A"),
        ("sync-b", "Sync B"),
        ("sync-c", "Sync C"),
    ]);

    // Initial submit
    let output = repo.submit(&bookmarks[2]);
    assert!(
        output.status.success(),
        "submit failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let pr_a = find_pr_number(&bookmarks[0]).expect("PR A");
    let pr_b = find_pr_number(&bookmarks[1]).expect("PR B");

    // Verify initial base
    assert_eq!(get_pr_base(pr_b), Some(bookmarks[0].clone()));

    // Merge PR A
    assert!(merge_pr(pr_a), "Failed to merge PR A");

    // Wait for merge to complete
    assert!(
        wait_for_pr_merged(pr_a, std::time::Duration::from_secs(30)).await,
        "Timed out waiting for PR A to merge"
    );

    // Run sync
    let sync_output = repo.sync();
    assert!(
        sync_output.status.success(),
        "sync failed: {}",
        String::from_utf8_lossy(&sync_output.stderr)
    );

    // PR B should now target main
    let new_base = get_pr_base(pr_b);
    assert_eq!(
        new_base,
        Some("main".into()),
        "PR B should target main after sync"
    );

    repo.cleanup();
}

#[tokio::test]
#[ignore = "E2E test requiring JJ_RYU_E2E_TESTS=1"]
async fn test_sync_multiple_stacks_updates_bases() {
    let Some(mut repo) = E2ERepo::new() else {
        eprintln!("Skipping: set JJ_RYU_E2E_TESTS=1");
        return;
    };

    // Create first stack: A -> B
    let stack1 = repo.build_stack(&[("multi-a", "Stack 1 A"), ("multi-b", "Stack 1 B")]);

    // Go back to main and create second stack: X -> Y
    let _ = Command::new("jj")
        .args(["new", "main"])
        .current_dir(repo.path())
        .output();

    let stack2 = repo.build_stack(&[("multi-x", "Stack 2 X"), ("multi-y", "Stack 2 Y")]);

    // Submit both stacks
    let output1 = repo.submit(&stack1[1]);
    assert!(output1.status.success(), "submit stack1 failed");

    let output2 = repo.submit(&stack2[1]);
    assert!(output2.status.success(), "submit stack2 failed");

    // Get PR numbers
    let pr_a = find_pr_number(&stack1[0]).expect("PR A");
    let pr_b = find_pr_number(&stack1[1]).expect("PR B");
    let pr_x = find_pr_number(&stack2[0]).expect("PR X");
    let pr_y = find_pr_number(&stack2[1]).expect("PR Y");

    // Verify initial bases: B->A, Y->X
    assert_eq!(
        get_pr_base(pr_b),
        Some(stack1[0].clone()),
        "B should initially target A"
    );
    assert_eq!(
        get_pr_base(pr_y),
        Some(stack2[0].clone()),
        "Y should initially target X"
    );

    // Merge both root PRs
    assert!(merge_pr(pr_a), "Failed to merge PR A");
    assert!(merge_pr(pr_x), "Failed to merge PR X");

    // Wait for merges to complete
    assert!(
        wait_for_pr_merged(pr_a, std::time::Duration::from_secs(30)).await,
        "Timed out waiting for PR A to merge"
    );
    assert!(
        wait_for_pr_merged(pr_x, std::time::Duration::from_secs(30)).await,
        "Timed out waiting for PR X to merge"
    );

    // Run sync to update bases
    let sync_output = repo.sync();
    assert!(
        sync_output.status.success(),
        "sync failed: {}",
        String::from_utf8_lossy(&sync_output.stderr)
    );

    // After sync: B and Y should target main (their parents were merged)
    assert_eq!(
        get_pr_base(pr_b),
        Some("main".into()),
        "PR B should target main after sync (A was merged)"
    );
    assert_eq!(
        get_pr_base(pr_y),
        Some("main".into()),
        "PR Y should target main after sync (X was merged)"
    );

    repo.cleanup();
}

#[tokio::test]
#[ignore = "E2E test requiring JJ_RYU_E2E_TESTS=1"]
async fn test_submit_dry_run() {
    let Some(mut repo) = E2ERepo::new() else {
        eprintln!("Skipping: set JJ_RYU_E2E_TESTS=1");
        return;
    };

    let bookmarks = repo.build_stack(&[("dry-a", "Dry A"), ("dry-b", "Dry B")]);

    // Dry run should not create PRs
    let output = repo.submit_dry_run(&bookmarks[1]);
    assert!(
        output.status.success(),
        "dry-run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // No PRs should exist
    assert!(
        find_pr_number(&bookmarks[0]).is_none(),
        "dry-run created PR"
    );
    assert!(
        find_pr_number(&bookmarks[1]).is_none(),
        "dry-run created PR"
    );

    // Now actually submit
    let output = repo.submit(&bookmarks[1]);
    assert!(
        output.status.success(),
        "submit failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // PRs should exist
    assert!(find_pr_number(&bookmarks[0]).is_some());
    assert!(find_pr_number(&bookmarks[1]).is_some());

    repo.cleanup();
}

// =============================================================================
// ExecutionStep Model E2E Tests (RFC: Unified Execution Step Model)
// =============================================================================

/// Test: Stack swap scenario works correctly with real GitHub API
///
/// This validates the critical swap scenario from the RFC where reordering
/// commits requires correct execution ordering to avoid GitHub API rejection.
#[tokio::test]
#[ignore = "E2E test requiring JJ_RYU_E2E_TESTS=1"]
async fn test_stack_swap_reorder() {
    let Some(mut repo) = E2ERepo::new() else {
        eprintln!("Skipping: set JJ_RYU_E2E_TESTS=1");
        return;
    };

    // Create initial stack: A -> B (A is root, B is leaf)
    let bookmarks = repo.build_stack(&[("swap-a", "Swap A"), ("swap-b", "Swap B")]);

    // Initial submit to create PRs with original structure
    let output = repo.submit(&bookmarks[1]);
    assert!(
        output.status.success(),
        "initial submit failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let pr_a = find_pr_number(&bookmarks[0]).expect("PR A should exist");
    let pr_b = find_pr_number(&bookmarks[1]).expect("PR B should exist");

    // Verify initial bases: A->main, B->A
    assert_eq!(get_pr_base(pr_a), Some("main".into()));
    assert_eq!(get_pr_base(pr_b), Some(bookmarks[0].clone()));

    // Swap the stack: rebase B before A (making B the new root)
    let rebase = Command::new("jj")
        .args(["rebase", "-r", &bookmarks[1], "--before", &bookmarks[0]])
        .current_dir(repo.path())
        .output()
        .expect("jj rebase");

    assert!(
        rebase.status.success(),
        "rebase failed: {}",
        String::from_utf8_lossy(&rebase.stderr)
    );

    // Re-submit after swap
    // The ExecutionStep model should handle this by:
    // 1. Retargeting B's PR from A to main (before pushing A's new history)
    // 2. Retargeting A's PR from main to B
    // 3. Pushing both branches
    let output = repo.submit(&bookmarks[0]); // A is now the leaf
    assert!(
        output.status.success(),
        "submit after swap failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify new bases after swap: B->main, A->B
    assert_eq!(
        get_pr_base(pr_b),
        Some("main".into()),
        "After swap, B should target main"
    );
    assert_eq!(
        get_pr_base(pr_a),
        Some(bookmarks[1].clone()),
        "After swap, A should target B"
    );

    repo.cleanup();
}

/// Test: Mixed operations (push, update, create) execute in correct order
#[tokio::test]
#[ignore = "E2E test requiring JJ_RYU_E2E_TESTS=1"]
async fn test_mixed_operations_ordering() {
    let Some(mut repo) = E2ERepo::new() else {
        eprintln!("Skipping: set JJ_RYU_E2E_TESTS=1");
        return;
    };

    // Create A -> B, submit only A first
    let bookmarks = repo.build_stack(&[("mixed-a", "Mixed A"), ("mixed-b", "Mixed B")]);

    // Submit just A (partial stack)
    let output = repo.submit(&bookmarks[0]);
    assert!(
        output.status.success(),
        "submit A failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let pr_a = find_pr_number(&bookmarks[0]).expect("PR A should exist");
    assert!(
        find_pr_number(&bookmarks[1]).is_none(),
        "PR B should not exist yet"
    );

    // Now add C after B and submit from C
    // This creates a scenario with mixed operations:
    // - A: exists, may need push if changed
    // - B: needs push + create
    // - C: needs push + create
    let _ = Command::new("jj")
        .args(["new", &bookmarks[1]])
        .current_dir(repo.path())
        .output();

    repo.create_commit("Mixed C");
    let c_bookmark = format!("{}-mixed-c", repo.prefix);
    let _ = Command::new("jj")
        .args(["bookmark", "create", &c_bookmark])
        .current_dir(repo.path())
        .output();
    repo.created_bookmarks.push(c_bookmark.clone());

    let output = repo.submit(&c_bookmark);
    assert!(
        output.status.success(),
        "submit C failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify all PRs exist with correct bases
    let pr_b = find_pr_number(&bookmarks[1]).expect("PR B should exist");
    let pr_c = find_pr_number(&c_bookmark).expect("PR C should exist");

    assert_eq!(get_pr_base(pr_a), Some("main".into()));
    assert_eq!(get_pr_base(pr_b), Some(bookmarks[0].clone()));
    assert_eq!(get_pr_base(pr_c), Some(bookmarks[1].clone()));

    repo.cleanup();
}

/// Test: Base update after inserting commit in middle of stack
#[tokio::test]
#[ignore = "E2E test requiring JJ_RYU_E2E_TESTS=1"]
async fn test_insert_middle_of_stack() {
    let Some(mut repo) = E2ERepo::new() else {
        eprintln!("Skipping: set JJ_RYU_E2E_TESTS=1");
        return;
    };

    // Create A -> C (skipping B initially)
    let _ = repo.build_stack(&[("insert-a", "Insert A")]);

    // Create C on top of A
    repo.create_commit("Insert C");
    let c_bookmark = format!("{}-insert-c", repo.prefix);
    let _ = Command::new("jj")
        .args(["bookmark", "create", &c_bookmark])
        .current_dir(repo.path())
        .output();
    repo.created_bookmarks.push(c_bookmark.clone());

    // Submit A -> C
    let output = repo.submit(&c_bookmark);
    assert!(
        output.status.success(),
        "initial submit failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Clone the bookmark name before further mutations
    let a_bookmark = repo.created_bookmarks[0].clone();
    let pr_a = find_pr_number(&a_bookmark).expect("PR A");
    let pr_c = find_pr_number(&c_bookmark).expect("PR C");

    // C should target A
    assert_eq!(get_pr_base(pr_c), Some(a_bookmark.clone()));

    // Now insert B between A and C using jj rebase
    // First, create B as a new commit
    let _ = Command::new("jj")
        .args(["new", &a_bookmark])
        .current_dir(repo.path())
        .output();

    repo.create_commit("Insert B");
    let b_bookmark = format!("{}-insert-b", repo.prefix);
    let _ = Command::new("jj")
        .args(["bookmark", "create", &b_bookmark])
        .current_dir(repo.path())
        .output();
    repo.created_bookmarks.push(b_bookmark.clone());

    // Rebase C onto B (so stack becomes A -> B -> C)
    let rebase = Command::new("jj")
        .args(["rebase", "-r", &c_bookmark, "--after", &b_bookmark])
        .current_dir(repo.path())
        .output()
        .expect("jj rebase");

    assert!(
        rebase.status.success(),
        "rebase C onto B failed: {}",
        String::from_utf8_lossy(&rebase.stderr)
    );

    // Re-submit from C
    let output = repo.submit(&c_bookmark);
    assert!(
        output.status.success(),
        "submit after insert failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify: A->main, B->A, C->B
    let pr_b = find_pr_number(&b_bookmark).expect("PR B should be created");

    assert_eq!(get_pr_base(pr_a), Some("main".into()));
    assert_eq!(get_pr_base(pr_b), Some(a_bookmark));
    assert_eq!(
        get_pr_base(pr_c),
        Some(b_bookmark),
        "C should now target B after insert"
    );

    repo.cleanup();
}
