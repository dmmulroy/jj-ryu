//! Change graph builder
//!
//! Builds a `ChangeGraph` from jj workspace state.
//! Uses single-stack semantics: only the stack from trunk to working copy.

use crate::error::Result;
use crate::repo::JjWorkspace;
use crate::types::{Bookmark, BookmarkSegment, BranchStack, ChangeGraph, LogEntry};
use std::collections::HashMap;
use tracing::debug;

/// Build a change graph from the current workspace state
///
/// This analyzes the single stack from trunk to working copy.
/// Only bookmarks between trunk and @ are included in the stack.
///
/// Returns a `ChangeGraph` with:
/// - `bookmarks`: All local bookmarks in the workspace (not just those in the stack).
///   This allows callers to validate bookmark existence before submission.
/// - `stack: Some(...)` if there are bookmarked commits between trunk and @
/// - `stack: None` if working copy is at trunk or no bookmarks exist
pub fn build_change_graph(workspace: &JjWorkspace) -> Result<ChangeGraph> {
    debug!("Building change graph from trunk to working copy...");

    // Query trunk()..@ to get all commits between trunk and working copy
    let changes = workspace.resolve_revset("trunk()..@")?;

    if changes.is_empty() {
        debug!("Working copy is at trunk, no stack to build");
        return Ok(ChangeGraph::default());
    }

    debug!("Found {} commits between trunk and @", changes.len());

    // Check for merge commits - we don't support them
    for change in &changes {
        if change.parents.len() > 1 {
            debug!("Found merge commit {} - excluding stack", change.commit_id);
            return Ok(ChangeGraph {
                bookmarks: HashMap::new(),
                stack: None,
                // Signals merge commit exclusion occurred, not actual count of excluded bookmarks
                excluded_bookmark_count: 1,
            });
        }
    }

    // Build segments from the changes
    // Changes are returned newest-first (working copy toward trunk)
    let (segments, bookmarks_by_name) = build_segments_from_changes(&changes, workspace)?;

    if segments.is_empty() {
        debug!("No bookmarked segments found");
        return Ok(ChangeGraph {
            bookmarks: bookmarks_by_name,
            stack: None,
            excluded_bookmark_count: 0,
        });
    }

    debug!("Built {} segments", segments.len());

    Ok(ChangeGraph {
        bookmarks: bookmarks_by_name,
        stack: Some(BranchStack { segments }),
        excluded_bookmark_count: 0,
    })
}

/// Build segments from a list of changes (newest-first order)
///
/// Returns segments in trunk-to-leaf order (reversed from input)
fn build_segments_from_changes(
    changes: &[LogEntry],
    workspace: &JjWorkspace,
) -> Result<(Vec<BookmarkSegment>, HashMap<String, Bookmark>)> {
    let all_bookmarks = workspace.local_bookmarks()?;
    let bookmarks_by_name: HashMap<String, Bookmark> = all_bookmarks
        .iter()
        .map(|b| (b.name.clone(), b.clone()))
        .collect();

    let mut segments: Vec<BookmarkSegment> = Vec::new();
    let mut current_changes: Vec<LogEntry> = Vec::new();

    // Process changes (newest to oldest, i.e., leaf toward trunk)
    for change in changes {
        // Every commit gets added to current_changes
        current_changes.push(change.clone());

        // If this commit has bookmarks, it's a segment boundary - complete the segment
        if change.local_bookmarks.is_empty() {
            continue;
        }

        // Collect bookmark objects
        let segment_bookmarks: Vec<Bookmark> = change
            .local_bookmarks
            .iter()
            .filter_map(|name| bookmarks_by_name.get(name).cloned())
            .collect();

        // Complete this segment
        if !segment_bookmarks.is_empty() {
            let changes_count = current_changes.len();
            segments.push(BookmarkSegment {
                bookmarks: segment_bookmarks,
                changes: std::mem::take(&mut current_changes),
            });

            debug!(
                "  Segment: [{}] with {} commits",
                change.local_bookmarks.join(", "),
                changes_count
            );
        }
    }

    // Any remaining unbookmarked commits at the base are dropped
    // (they have no bookmark to submit)
    if !current_changes.is_empty() {
        debug!(
            "  Dropping {} unbookmarked commits at base of stack",
            current_changes.len()
        );
    }

    // Reverse to get trunk-to-leaf order
    segments.reverse();

    Ok((segments, bookmarks_by_name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_log_entry(commit_id: &str, change_id: &str, bookmarks: Vec<&str>) -> LogEntry {
        LogEntry {
            commit_id: commit_id.to_string(),
            change_id: change_id.to_string(),
            author_name: "Test".to_string(),
            author_email: "test@test.com".to_string(),
            description_first_line: format!("Commit {commit_id}"),
            parents: vec!["parent".to_string()],
            local_bookmarks: bookmarks.into_iter().map(String::from).collect(),
            remote_bookmarks: vec![],
            is_working_copy: false,
            authored_at: Utc::now(),
            committed_at: Utc::now(),
        }
    }

    fn make_bookmark(name: &str, commit_id: &str, change_id: &str) -> Bookmark {
        Bookmark {
            name: name.to_string(),
            commit_id: commit_id.to_string(),
            change_id: change_id.to_string(),
            has_remote: false,
            is_synced: false,
        }
    }

    #[test]
    fn test_single_bookmark_segment() {
        // Simulate: trunk <- commit1 (feat-a) <- commit2 (@)
        // Changes come newest-first: [commit2, commit1]
        let changes = vec![
            make_log_entry("commit2", "change2", vec![]),
            make_log_entry("commit1", "change1", vec!["feat-a"]),
        ];

        let bookmarks: HashMap<String, Bookmark> = [(
            "feat-a".to_string(),
            make_bookmark("feat-a", "commit1", "change1"),
        )]
        .into();

        // Manual segment building for test
        let mut segments = Vec::new();
        let mut current_changes = Vec::new();

        for change in &changes {
            current_changes.push(change.clone());
            if !change.local_bookmarks.is_empty() {
                let bms: Vec<Bookmark> = change
                    .local_bookmarks
                    .iter()
                    .filter_map(|n| bookmarks.get(n).cloned())
                    .collect();
                segments.push(BookmarkSegment {
                    bookmarks: bms,
                    changes: std::mem::take(&mut current_changes),
                });
            }
        }
        segments.reverse();

        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].bookmarks[0].name, "feat-a");
        assert_eq!(segments[0].changes.len(), 2);
    }

    #[test]
    fn test_two_bookmark_stack() {
        // Simulate: trunk <- c1 (feat-a) <- c2 (feat-b) <- c3 (@)
        // Changes newest-first: [c3, c2, c1]
        let changes = vec![
            make_log_entry("c3", "ch3", vec![]),
            make_log_entry("c2", "ch2", vec!["feat-b"]),
            make_log_entry("c1", "ch1", vec!["feat-a"]),
        ];

        let bookmarks: HashMap<String, Bookmark> = [
            ("feat-a".to_string(), make_bookmark("feat-a", "c1", "ch1")),
            ("feat-b".to_string(), make_bookmark("feat-b", "c2", "ch2")),
        ]
        .into();

        let mut segments = Vec::new();
        let mut current_changes = Vec::new();

        for change in &changes {
            current_changes.push(change.clone());
            if !change.local_bookmarks.is_empty() {
                let bms: Vec<Bookmark> = change
                    .local_bookmarks
                    .iter()
                    .filter_map(|n| bookmarks.get(n).cloned())
                    .collect();
                segments.push(BookmarkSegment {
                    bookmarks: bms,
                    changes: std::mem::take(&mut current_changes),
                });
            }
        }
        segments.reverse();

        assert_eq!(segments.len(), 2);
        // After reverse: [feat-a, feat-b] (trunk to leaf order)
        assert_eq!(segments[0].bookmarks[0].name, "feat-a");
        assert_eq!(segments[1].bookmarks[0].name, "feat-b");
    }
}
