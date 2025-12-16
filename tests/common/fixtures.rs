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
/// Returns a `ChangeGraph` with properly connected segments.
pub fn make_linear_stack(names: &[&str]) -> ChangeGraph {
    let mut bookmarks = HashMap::new();
    let mut bookmark_to_change_id = HashMap::new();
    let mut adjacency = HashMap::new();
    let mut change_to_segment = HashMap::new();
    let mut segments = Vec::new();

    for (i, name) in names.iter().enumerate() {
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
        bookmark_to_change_id.insert(name.to_string(), change_id.clone());
        change_to_segment.insert(change_id.clone(), vec![log_entry.clone()]);

        // Link to parent (previous bookmark's change_id)
        if i > 0 {
            let parent_change_id = format!("{}_change", names[i - 1]);
            adjacency.insert(change_id.clone(), parent_change_id);
        }

        segments.push(BookmarkSegment {
            bookmarks: vec![bm],
            changes: vec![log_entry],
        });
    }

    let leaf_id = format!("{}_change", names.last().unwrap());
    let root_id = format!("{}_change", names[0]);

    ChangeGraph {
        bookmarks,
        bookmark_to_change_id,
        bookmarked_change_adjacency_list: adjacency,
        bookmarked_change_id_to_segment: change_to_segment,
        stack_leafs: [leaf_id].into_iter().collect(),
        stack_roots: [root_id].into_iter().collect(),
        stacks: vec![BranchStack { segments }],
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

    let bookmark_to_change_id: HashMap<String, String> = names
        .iter()
        .map(|name| (name.to_string(), change_id.clone()))
        .collect();

    let log_entry =
        make_log_entry_with_ids("Shared commit", &commit_id, &change_id, names);

    let segment = BookmarkSegment {
        bookmarks: names
            .iter()
            .map(|n| make_bookmark_with_ids(n, &commit_id, &change_id))
            .collect(),
        changes: vec![log_entry.clone()],
    };

    let mut change_to_segment = HashMap::new();
    change_to_segment.insert(change_id.clone(), vec![log_entry]);

    ChangeGraph {
        bookmarks,
        bookmark_to_change_id,
        bookmarked_change_adjacency_list: HashMap::new(),
        bookmarked_change_id_to_segment: change_to_segment,
        stack_leafs: [change_id.clone()].into_iter().collect(),
        stack_roots: [change_id].into_iter().collect(),
        stacks: vec![BranchStack {
            segments: vec![segment],
        }],
        excluded_bookmark_count: 0,
    }
}
