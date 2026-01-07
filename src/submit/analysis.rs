//! Phase 1: Submission analysis
//!
//! Identifies what needs to be submitted for a given target bookmark.

use crate::error::{Error, Result};
use crate::types::{Bookmark, BookmarkSegment, ChangeGraph, NarrowedBookmarkSegment};

/// Result of submission analysis
#[derive(Debug, Clone)]
pub struct SubmissionAnalysis {
    /// Target bookmark name
    pub target_bookmark: String,
    /// Segments to submit (from trunk towards target), each narrowed to one bookmark
    pub segments: Vec<NarrowedBookmarkSegment>,
}

/// Analyze what needs to be submitted for a given bookmark
///
/// Works with single-stack semantics: the graph contains only one stack
/// from trunk to working copy. If `target_bookmark` is None, submits the
/// entire stack (leaf bookmark). If specified, submits up to that bookmark.
pub fn analyze_submission(
    graph: &ChangeGraph,
    target_bookmark: Option<&str>,
) -> Result<SubmissionAnalysis> {
    let stack = graph
        .stack
        .as_ref()
        .ok_or_else(|| Error::NoStack("No bookmarks found between trunk and working copy. Create a bookmark with: jj bookmark create <name>".to_string()))?;

    if stack.segments.is_empty() {
        return Err(Error::NoStack("Stack has no segments".to_string()));
    }

    // Determine target index
    let target_index = if let Some(target) = target_bookmark {
        stack
            .segments
            .iter()
            .position(|segment| segment.bookmarks.iter().any(|b| b.name == target))
            .ok_or_else(|| Error::BookmarkNotFound(target.to_string()))?
    } else {
        // No target specified - use leaf (last segment)
        stack.segments.len() - 1
    };

    // Get segments from trunk (index 0) to target (inclusive)
    let relevant_segments = &stack.segments[0..=target_index];

    // Narrow each segment to a single bookmark using heuristics
    let narrowed: Vec<NarrowedBookmarkSegment> = relevant_segments
        .iter()
        .map(|segment| {
            let bookmark = select_bookmark_for_segment(segment, target_bookmark);

            NarrowedBookmarkSegment {
                bookmark,
                changes: segment.changes.clone(),
            }
        })
        .collect();

    // Use the actual selected bookmark name for the target
    let actual_target = narrowed
        .last()
        .map(|s| s.bookmark.name.clone())
        .unwrap_or_default();

    Ok(SubmissionAnalysis {
        target_bookmark: actual_target,
        segments: narrowed,
    })
}

/// Select a single bookmark from a segment using heuristics
///
/// Selection priority:
/// 1. If target is specified and present, use it
/// 2. Exclude temporary bookmarks (wip, tmp, backup, -old)
/// 3. Prefer shorter names (more likely to be "canonical")
/// 4. Fall back to alphabetically first
pub fn select_bookmark_for_segment(segment: &BookmarkSegment, target: Option<&str>) -> Bookmark {
    let bookmarks = &segment.bookmarks;

    // Single bookmark - no selection needed
    if bookmarks.len() == 1 {
        return bookmarks[0].clone();
    }

    // 1. Prefer target if specified and present
    if let Some(target_name) = target {
        if let Some(b) = bookmarks.iter().find(|b| b.name == target_name) {
            return b.clone();
        }
    }

    // 2. Filter out temporary bookmarks
    let candidates: Vec<_> = bookmarks
        .iter()
        .filter(|b| !is_temporary_bookmark(&b.name))
        .collect();

    let pool: Vec<&Bookmark> = if candidates.is_empty() {
        bookmarks.iter().collect()
    } else {
        candidates
    };

    // 3. Prefer shorter names, then alphabetically first
    pool.into_iter()
        .min_by(|a, b| match a.name.len().cmp(&b.name.len()) {
            std::cmp::Ordering::Equal => a.name.cmp(&b.name),
            other => other,
        })
        .cloned()
        .unwrap_or_else(|| bookmarks[0].clone())
}

/// Check if a bookmark name appears to be temporary
fn is_temporary_bookmark(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("wip")
        || lower.contains("tmp")
        || lower.contains("temp")
        || lower.contains("backup")
        || lower.ends_with("-old")
        || lower.ends_with("_old")
        || lower.starts_with("wip-")
        || lower.starts_with("wip/")
}

/// Get the expected base branch for a bookmark in a submission
///
/// Returns the bookmark name that this bookmark should be based on,
/// or the default branch name if it's the first in the stack.
pub fn get_base_branch(
    bookmark_name: &str,
    segments: &[NarrowedBookmarkSegment],
    default_branch: &str,
) -> Result<String> {
    for (i, segment) in segments.iter().enumerate() {
        if segment.bookmark.name == bookmark_name {
            if i == 0 {
                // First segment is based on default branch
                return Ok(default_branch.to_string());
            }
            // Otherwise, based on previous segment's bookmark
            return Ok(segments[i - 1].bookmark.name.clone());
        }
    }

    Err(Error::BookmarkNotFound(bookmark_name.to_string()))
}

/// Generate a PR title from the bookmark's commits
///
/// Uses the oldest (root) commit's description as the title, since that
/// typically represents the primary intent of the change. Falls back to
/// bookmark name if no description is available.
pub fn generate_pr_title(
    bookmark_name: &str,
    segments: &[NarrowedBookmarkSegment],
) -> Result<String> {
    let segment = segments
        .iter()
        .find(|s| s.bookmark.name == bookmark_name)
        .ok_or_else(|| Error::BookmarkNotFound(bookmark_name.to_string()))?;

    if segment.changes.is_empty() {
        return Ok(bookmark_name.to_string());
    }

    // Use the oldest (root) commit's description as the title
    // changes[0] is newest, changes[last] is oldest/root
    let root_commit = segment
        .changes
        .last()
        .expect("segment has at least one change");
    let title = &root_commit.description_first_line;
    if title.is_empty() {
        Ok(bookmark_name.to_string())
    } else {
        Ok(title.clone())
    }
}

/// Create narrowed segments from resolved bookmarks and analysis
///
/// This bridges CLI bookmark selection with submission planning.
pub fn create_narrowed_segments(
    resolved_bookmarks: &[Bookmark],
    analysis: &SubmissionAnalysis,
) -> Result<Vec<NarrowedBookmarkSegment>> {
    let mut segments = Vec::new();

    for (i, bookmark) in resolved_bookmarks.iter().enumerate() {
        let corresponding_segment = analysis
            .segments
            .get(i)
            .ok_or_else(|| Error::Internal(format!("No segment at index {i}")))?;

        segments.push(NarrowedBookmarkSegment {
            bookmark: bookmark.clone(),
            changes: corresponding_segment.changes.clone(),
        });
    }

    Ok(segments)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BookmarkSegment, BranchStack, LogEntry};
    use chrono::Utc;

    fn make_bookmark(name: &str) -> Bookmark {
        Bookmark {
            name: name.to_string(),
            commit_id: format!("{name}_commit"),
            change_id: format!("{name}_change"),
            has_remote: false,
            is_synced: false,
        }
    }

    fn make_log_entry(desc: &str, bookmarks: &[&str]) -> LogEntry {
        LogEntry {
            commit_id: format!("{desc}_commit"),
            change_id: format!("{desc}_change"),
            author_name: "Test".to_string(),
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

    #[test]
    fn test_analyze_submission_finds_target() {
        let bm1 = make_bookmark("feat-a");
        let bm2 = make_bookmark("feat-b");

        let stack = BranchStack {
            segments: vec![
                BookmarkSegment {
                    bookmarks: vec![bm1.clone()],
                    changes: vec![make_log_entry("First change", &["feat-a"])],
                },
                BookmarkSegment {
                    bookmarks: vec![bm2.clone()],
                    changes: vec![make_log_entry("Second change", &["feat-b"])],
                },
            ],
        };

        let graph = ChangeGraph {
            bookmarks: [("feat-a".to_string(), bm1), ("feat-b".to_string(), bm2)]
                .into_iter()
                .collect(),
            stack: Some(stack),
            excluded_bookmark_count: 0,
        };

        let analysis = analyze_submission(&graph, Some("feat-b")).unwrap();
        assert_eq!(analysis.target_bookmark, "feat-b");
        assert_eq!(analysis.segments.len(), 2);
        assert_eq!(analysis.segments[0].bookmark.name, "feat-a");
        assert_eq!(analysis.segments[1].bookmark.name, "feat-b");
    }

    #[test]
    fn test_analyze_submission_no_target_uses_leaf() {
        let bm1 = make_bookmark("feat-a");
        let bm2 = make_bookmark("feat-b");

        let stack = BranchStack {
            segments: vec![
                BookmarkSegment {
                    bookmarks: vec![bm1.clone()],
                    changes: vec![make_log_entry("First change", &["feat-a"])],
                },
                BookmarkSegment {
                    bookmarks: vec![bm2.clone()],
                    changes: vec![make_log_entry("Second change", &["feat-b"])],
                },
            ],
        };

        let graph = ChangeGraph {
            bookmarks: [("feat-a".to_string(), bm1), ("feat-b".to_string(), bm2)]
                .into_iter()
                .collect(),
            stack: Some(stack),
            excluded_bookmark_count: 0,
        };

        // No target - should use leaf (feat-b)
        let analysis = analyze_submission(&graph, None).unwrap();
        assert_eq!(analysis.target_bookmark, "feat-b");
        assert_eq!(analysis.segments.len(), 2);
    }

    #[test]
    fn test_analyze_submission_no_stack() {
        let graph = ChangeGraph::default();
        let result = analyze_submission(&graph, None);
        assert!(matches!(result, Err(Error::NoStack(_))));
    }

    #[test]
    fn test_analyze_submission_bookmark_not_found() {
        let bm1 = make_bookmark("feat-a");

        let stack = BranchStack {
            segments: vec![BookmarkSegment {
                bookmarks: vec![bm1.clone()],
                changes: vec![make_log_entry("First change", &["feat-a"])],
            }],
        };

        let graph = ChangeGraph {
            bookmarks: std::iter::once(("feat-a".to_string(), bm1)).collect(),
            stack: Some(stack),
            excluded_bookmark_count: 0,
        };

        let result = analyze_submission(&graph, Some("nonexistent"));
        assert!(matches!(result, Err(Error::BookmarkNotFound(_))));
    }

    #[test]
    fn test_get_base_branch_first() {
        let segments = vec![NarrowedBookmarkSegment {
            bookmark: make_bookmark("feat-a"),
            changes: vec![],
        }];

        let base = get_base_branch("feat-a", &segments, "main").unwrap();
        assert_eq!(base, "main");
    }

    #[test]
    fn test_get_base_branch_stacked() {
        let segments = vec![
            NarrowedBookmarkSegment {
                bookmark: make_bookmark("feat-a"),
                changes: vec![],
            },
            NarrowedBookmarkSegment {
                bookmark: make_bookmark("feat-b"),
                changes: vec![],
            },
        ];

        let base = get_base_branch("feat-b", &segments, "main").unwrap();
        assert_eq!(base, "feat-a");
    }

    #[test]
    fn test_generate_pr_title() {
        let segments = vec![NarrowedBookmarkSegment {
            bookmark: make_bookmark("feat-a"),
            changes: vec![make_log_entry("Add cool feature", &["feat-a"])],
        }];

        let title = generate_pr_title("feat-a", &segments).unwrap();
        assert_eq!(title, "Add cool feature");
    }

    #[test]
    fn test_generate_pr_title_empty_fallback() {
        let segments = vec![NarrowedBookmarkSegment {
            bookmark: make_bookmark("feat-a"),
            changes: vec![make_log_entry("", &["feat-a"])],
        }];

        let title = generate_pr_title("feat-a", &segments).unwrap();
        assert_eq!(title, "feat-a");
    }

    #[test]
    fn test_generate_pr_title_uses_root_commit() {
        // changes[0] is newest, changes[last] is oldest (root)
        let segments = vec![NarrowedBookmarkSegment {
            bookmark: make_bookmark("feat-a"),
            changes: vec![
                make_log_entry("Fix typo in feature", &["feat-a"]), // newest
                make_log_entry("Add tests for feature", &[]),       // middle
                make_log_entry("Implement cool feature", &[]),      // oldest (root)
            ],
        }];

        let title = generate_pr_title("feat-a", &segments).unwrap();
        // Should use the root commit's description, not the latest
        assert_eq!(title, "Implement cool feature");
    }

    #[test]
    fn test_select_bookmark_single() {
        let segment = BookmarkSegment {
            bookmarks: vec![make_bookmark("feat-a")],
            changes: vec![],
        };

        let selected = select_bookmark_for_segment(&segment, None);
        assert_eq!(selected.name, "feat-a");
    }

    #[test]
    fn test_select_bookmark_prefers_target() {
        let segment = BookmarkSegment {
            bookmarks: vec![make_bookmark("feat-a"), make_bookmark("feat-b")],
            changes: vec![],
        };

        let selected = select_bookmark_for_segment(&segment, Some("feat-b"));
        assert_eq!(selected.name, "feat-b");
    }

    #[test]
    fn test_select_bookmark_excludes_wip() {
        let segment = BookmarkSegment {
            bookmarks: vec![make_bookmark("feat-a-wip"), make_bookmark("feat-a")],
            changes: vec![],
        };

        let selected = select_bookmark_for_segment(&segment, None);
        assert_eq!(selected.name, "feat-a");
    }

    #[test]
    fn test_select_bookmark_excludes_tmp() {
        let segment = BookmarkSegment {
            bookmarks: vec![make_bookmark("tmp-test"), make_bookmark("feature")],
            changes: vec![],
        };

        let selected = select_bookmark_for_segment(&segment, None);
        assert_eq!(selected.name, "feature");
    }

    #[test]
    fn test_select_bookmark_excludes_backup() {
        let segment = BookmarkSegment {
            bookmarks: vec![make_bookmark("feat-backup"), make_bookmark("feat")],
            changes: vec![],
        };

        let selected = select_bookmark_for_segment(&segment, None);
        assert_eq!(selected.name, "feat");
    }

    #[test]
    fn test_select_bookmark_excludes_old_suffix() {
        let segment = BookmarkSegment {
            bookmarks: vec![make_bookmark("feat-old"), make_bookmark("feat")],
            changes: vec![],
        };

        let selected = select_bookmark_for_segment(&segment, None);
        assert_eq!(selected.name, "feat");
    }

    #[test]
    fn test_select_bookmark_prefers_shorter() {
        let segment = BookmarkSegment {
            bookmarks: vec![
                make_bookmark("feature-implementation"),
                make_bookmark("feat"),
            ],
            changes: vec![],
        };

        let selected = select_bookmark_for_segment(&segment, None);
        assert_eq!(selected.name, "feat");
    }

    #[test]
    fn test_select_bookmark_alphabetical_tiebreaker() {
        // Same length names - should pick alphabetically first
        let segment = BookmarkSegment {
            bookmarks: vec![make_bookmark("beta1"), make_bookmark("alpha")],
            changes: vec![],
        };

        let selected = select_bookmark_for_segment(&segment, None);
        assert_eq!(selected.name, "alpha");
    }

    #[test]
    fn test_select_bookmark_prefers_shorter_over_alphabetical() {
        // Different length names - should pick shorter even if not alphabetically first
        let segment = BookmarkSegment {
            bookmarks: vec![make_bookmark("alpha"), make_bookmark("beta")],
            changes: vec![],
        };

        let selected = select_bookmark_for_segment(&segment, None);
        assert_eq!(selected.name, "beta"); // shorter (4) beats alpha (5)
    }

    #[test]
    fn test_select_bookmark_all_temporary_falls_back() {
        let segment = BookmarkSegment {
            bookmarks: vec![make_bookmark("wip-a"), make_bookmark("tmp-b")],
            changes: vec![],
        };

        // Should still select something even if all are "temporary"
        let selected = select_bookmark_for_segment(&segment, None);
        assert_eq!(selected.name, "tmp-b"); // shorter, then alphabetical
    }

    #[test]
    fn test_is_temporary_bookmark() {
        assert!(is_temporary_bookmark("feat-wip"));
        assert!(is_temporary_bookmark("WIP-feature"));
        assert!(is_temporary_bookmark("wip/test"));
        assert!(is_temporary_bookmark("tmp-test"));
        assert!(is_temporary_bookmark("temp-feature"));
        assert!(is_temporary_bookmark("my-backup"));
        assert!(is_temporary_bookmark("feat-old"));
        assert!(is_temporary_bookmark("feat_old"));

        assert!(!is_temporary_bookmark("feature"));
        assert!(!is_temporary_bookmark("my-feat"));
        assert!(!is_temporary_bookmark("gold-feature")); // contains "old" but not suffix
    }
}
