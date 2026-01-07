//! Test data factories for jj-ryu types
//!
//! These are test utilities - not all may be used in current tests but are
//! available for future test development.

#![allow(dead_code)]

use chrono::Utc;
use jj_ryu::types::{
    Bookmark, BookmarkSegment, BranchStack, ChangeGraph, LogEntry, Platform, PlatformConfig,
    PrComment, PullRequest,
};
use std::collections::HashMap;

/// Create a bookmark with default values
pub fn make_bookmark(name: &str) -> Bookmark {
    Bookmark {
        name: name.to_string(),
        commit_id: format!("{name}_commit_abc123"),
        change_id: format!("{name}_change_xyz789"),
        has_remote: false,
        is_synced: false,
    }
}

/// Create a bookmark that is synced with remote
pub fn make_bookmark_synced(name: &str) -> Bookmark {
    Bookmark {
        has_remote: true,
        is_synced: true,
        ..make_bookmark(name)
    }
}

/// Create a bookmark with specific commit/change IDs
pub fn make_bookmark_with_ids(name: &str, commit_id: &str, change_id: &str) -> Bookmark {
    Bookmark {
        name: name.to_string(),
        commit_id: commit_id.to_string(),
        change_id: change_id.to_string(),
        has_remote: false,
        is_synced: false,
    }
}

/// Create a log entry with specific IDs
pub fn make_log_entry_with_ids(
    desc: &str,
    commit_id: &str,
    change_id: &str,
    bookmarks: &[&str],
) -> LogEntry {
    LogEntry {
        commit_id: commit_id.to_string(),
        change_id: change_id.to_string(),
        author_name: "Test Author".to_string(),
        author_email: "test@example.com".to_string(),
        description_first_line: desc.to_string(),
        parents: vec![],
        local_bookmarks: bookmarks.iter().map(ToString::to_string).collect(),
        remote_bookmarks: vec![],
        is_working_copy: false,
        authored_at: Utc::now(),
        committed_at: Utc::now(),
    }
}

/// Create a pull request with default values
pub fn make_pr(number: u64, head: &str, base: &str) -> PullRequest {
    PullRequest {
        number,
        html_url: format!("https://github.com/test/repo/pull/{number}"),
        base_ref: base.to_string(),
        head_ref: head.to_string(),
        title: format!("PR for {head}"),
        node_id: Some(format!("PR_node_{number}")),
        is_draft: false,
    }
}

/// Create a draft pull request
pub fn make_pr_draft(number: u64, head: &str, base: &str) -> PullRequest {
    PullRequest {
        number,
        html_url: format!("https://github.com/test/repo/pull/{number}"),
        base_ref: base.to_string(),
        head_ref: head.to_string(),
        title: format!("PR for {head}"),
        node_id: Some(format!("PR_node_{number}")),
        is_draft: true,
    }
}

/// Create a PR comment
pub fn make_pr_comment(id: u64, body: &str) -> PrComment {
    PrComment {
        id,
        body: body.to_string(),
    }
}

/// Create a GitHub platform config
pub fn github_config() -> PlatformConfig {
    PlatformConfig {
        platform: Platform::GitHub,
        owner: "testowner".to_string(),
        repo: "testrepo".to_string(),
        host: None,
    }
}

/// Create a GitLab platform config
pub fn gitlab_config() -> PlatformConfig {
    PlatformConfig {
        platform: Platform::GitLab,
        owner: "testowner".to_string(),
        repo: "testrepo".to_string(),
        host: None,
    }
}

/// Build a linear stack graph: trunk -> bm1 -> bm2 -> bm3
///
/// Returns a `ChangeGraph` with a single stack containing the given bookmarks.
pub fn make_linear_stack(names: &[&str]) -> ChangeGraph {
    let mut bookmarks = HashMap::new();
    let mut segments = Vec::new();

    for name in names {
        let change_id = format!("{name}_change");
        let commit_id = format!("{name}_commit");

        let bm = make_bookmark_with_ids(name, &commit_id, &change_id);
        let log_entry = make_log_entry_with_ids(
            &format!("Commit for {name}"),
            &commit_id,
            &change_id,
            &[name],
        );

        bookmarks.insert(name.to_string(), bm.clone());

        segments.push(BookmarkSegment {
            bookmarks: vec![bm],
            changes: vec![log_entry],
        });
    }

    ChangeGraph {
        bookmarks,
        stack: Some(BranchStack { segments }),
        excluded_bookmark_count: 0,
    }
}

/// Build a graph with multiple bookmarks pointing to the same commit
pub fn make_multi_bookmark_segment(names: &[&str]) -> ChangeGraph {
    let change_id = "shared_change".to_string();
    let commit_id = "shared_commit".to_string();

    let bookmarks: HashMap<String, Bookmark> = names
        .iter()
        .map(|name| {
            (
                name.to_string(),
                make_bookmark_with_ids(name, &commit_id, &change_id),
            )
        })
        .collect();

    let log_entry = make_log_entry_with_ids("Shared commit", &commit_id, &change_id, names);

    let segment = BookmarkSegment {
        bookmarks: names
            .iter()
            .map(|n| make_bookmark_with_ids(n, &commit_id, &change_id))
            .collect(),
        changes: vec![log_entry],
    };

    ChangeGraph {
        bookmarks,
        stack: Some(BranchStack {
            segments: vec![segment],
        }),
        excluded_bookmark_count: 0,
    }
}
