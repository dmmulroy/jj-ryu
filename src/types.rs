//! Core types for jj-ryu

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// A jj bookmark (branch reference)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Bookmark {
    /// Bookmark name
    pub name: String,
    /// Git commit ID (hex)
    pub commit_id: String,
    /// jj change ID (hex)
    pub change_id: String,
    /// Whether this bookmark exists on any remote
    pub has_remote: bool,
    /// Whether local and remote are in sync
    pub is_synced: bool,
}

/// A commit/change entry from jj log
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// Git commit ID (hex)
    pub commit_id: String,
    /// jj change ID (hex)
    pub change_id: String,
    /// Author name
    pub author_name: String,
    /// Author email
    pub author_email: String,
    /// First line of commit description
    pub description_first_line: String,
    /// Parent commit IDs
    pub parents: Vec<String>,
    /// Local bookmarks pointing to this commit
    pub local_bookmarks: Vec<String>,
    /// Remote bookmarks pointing to this commit (format: "name@remote")
    pub remote_bookmarks: Vec<String>,
    /// Whether this is the working copy commit
    pub is_working_copy: bool,
    /// When the commit was authored
    pub authored_at: DateTime<Utc>,
    /// When the commit was committed
    pub committed_at: DateTime<Utc>,
}

/// A segment of changes belonging to one or more bookmarks
#[derive(Debug, Clone)]
pub struct BookmarkSegment {
    /// Bookmarks pointing to the tip of this segment
    pub bookmarks: Vec<Bookmark>,
    /// Changes in this segment (newest first)
    pub changes: Vec<LogEntry>,
}

/// A segment narrowed to a single bookmark (after user selection)
#[derive(Debug, Clone)]
pub struct NarrowedBookmarkSegment {
    /// The selected bookmark for this segment
    pub bookmark: Bookmark,
    /// Changes in this segment (newest first)
    pub changes: Vec<LogEntry>,
}

/// A stack of bookmarks from trunk to a leaf
#[derive(Debug, Clone)]
pub struct BranchStack {
    /// Segments from trunk (index 0) to leaf (last index)
    pub segments: Vec<BookmarkSegment>,
}

/// The complete change graph for a repository
#[derive(Debug, Clone, Default)]
pub struct ChangeGraph {
    /// All bookmarks by name
    pub bookmarks: HashMap<String, Bookmark>,
    /// Map from bookmark name to change ID
    pub bookmark_to_change_id: HashMap<String, String>,
    /// Adjacency list: child change ID -> parent change ID (for stacking)
    pub bookmarked_change_adjacency_list: HashMap<String, String>,
    /// Map from bookmarked change ID to the commits in that segment
    pub bookmarked_change_id_to_segment: HashMap<String, Vec<LogEntry>>,
    /// Change IDs that are leaf nodes (no children)
    pub stack_leafs: HashSet<String>,
    /// Change IDs that are root nodes (directly on trunk)
    pub stack_roots: HashSet<String>,
    /// All detected stacks
    pub stacks: Vec<BranchStack>,
    /// Number of bookmarks excluded due to merge commits
    pub excluded_bookmark_count: usize,
}

/// A pull request / merge request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    /// PR/MR number
    pub number: u64,
    /// Web URL for the PR/MR
    pub html_url: String,
    /// Base branch name
    pub base_ref: String,
    /// Head branch name
    pub head_ref: String,
    /// PR/MR title
    pub title: String,
    /// GraphQL node ID (GitHub only, used for mutations)
    pub node_id: Option<String>,
    /// Whether PR is a draft
    pub is_draft: bool,
}

/// A comment on a pull request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrComment {
    /// Comment ID
    pub id: u64,
    /// Comment body text
    pub body: String,
}

/// A git remote
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitRemote {
    /// Remote name (e.g., "origin")
    pub name: String,
    /// Remote URL
    pub url: String,
}

/// Detected platform type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Platform {
    /// GitHub or GitHub Enterprise
    GitHub,
    /// GitLab or self-hosted GitLab
    GitLab,
}

/// Platform configuration
#[derive(Debug, Clone)]
pub struct PlatformConfig {
    /// Platform type
    pub platform: Platform,
    /// Repository owner (user or organization)
    pub owner: String,
    /// Repository name
    pub repo: String,
    /// Custom host (None for github.com/gitlab.com)
    pub host: Option<String>,
}
