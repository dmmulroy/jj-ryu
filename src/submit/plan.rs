//! Phase 2: Submission planning
//!
//! Determines what operations need to be performed to submit a stack.

use crate::error::Result;
use crate::platform::PlatformService;
use crate::submit::SubmissionAnalysis;
use crate::submit::analysis::{generate_pr_title, get_base_branch};
use crate::types::{Bookmark, NarrowedBookmarkSegment, PullRequest};
use std::collections::HashMap;

/// Information about a PR that needs to be created
#[derive(Debug, Clone)]
pub struct PrToCreate {
    /// Bookmark for this PR
    pub bookmark: Bookmark,
    /// Base branch (previous bookmark or default branch)
    pub base_branch: String,
    /// Generated PR title
    pub title: String,
    /// Whether to create as draft
    pub draft: bool,
}

/// Information about a PR that needs its base updated
#[derive(Debug, Clone)]
pub struct PrBaseUpdate {
    /// Bookmark for this PR
    pub bookmark: Bookmark,
    /// Current base branch
    pub current_base: String,
    /// Expected base branch
    pub expected_base: String,
    /// Existing PR
    pub pr: PullRequest,
}

/// Submission plan
#[derive(Debug, Clone)]
pub struct SubmissionPlan {
    /// Segments to submit
    pub segments: Vec<NarrowedBookmarkSegment>,
    /// Bookmarks that need to be pushed to remote
    pub bookmarks_needing_push: Vec<Bookmark>,
    /// Bookmarks that need new PRs created
    pub prs_to_create: Vec<PrToCreate>,
    /// Bookmarks with existing PRs that need base updated
    pub prs_to_update_base: Vec<PrBaseUpdate>,
    /// Draft PRs that should be published
    pub prs_to_publish: Vec<PullRequest>,
    /// Existing PRs by bookmark name
    pub existing_prs: HashMap<String, PullRequest>,
    /// Remote name to push to
    pub remote: String,
    /// Default branch name (main/master)
    pub default_branch: String,
}

/// Create a submission plan
///
/// This determines what operations need to be performed:
/// - Which bookmarks need pushing
/// - Which PRs need to be created
/// - Which PR bases need updating
pub async fn create_submission_plan(
    analysis: &SubmissionAnalysis,
    platform: &dyn PlatformService,
    remote: &str,
    default_branch: &str,
) -> Result<SubmissionPlan> {
    let segments = &analysis.segments;
    let bookmarks: Vec<&Bookmark> = segments.iter().map(|s| &s.bookmark).collect();

    // Check for existing PRs
    let mut existing_prs = HashMap::new();
    for bookmark in &bookmarks {
        if let Some(pr) = platform.find_existing_pr(&bookmark.name).await? {
            existing_prs.insert(bookmark.name.clone(), pr);
        }
    }

    // Determine what needs to happen
    let mut bookmarks_needing_push = Vec::new();
    let mut prs_to_create = Vec::new();
    let mut prs_to_update_base = Vec::new();

    for bookmark in &bookmarks {
        // Check if needs push
        if !bookmark.has_remote || !bookmark.is_synced {
            bookmarks_needing_push.push((*bookmark).clone());
        }

        // Check if needs PR creation
        if let Some(pr) = existing_prs.get(&bookmark.name) {
            // PR exists - check if base needs updating
            let expected_base = get_base_branch(&bookmark.name, segments, default_branch)?;

            if pr.base_ref != expected_base {
                prs_to_update_base.push(PrBaseUpdate {
                    bookmark: (*bookmark).clone(),
                    current_base: pr.base_ref.clone(),
                    expected_base,
                    pr: pr.clone(),
                });
            }
        } else {
            // PR doesn't exist - needs creation
            let base_branch = get_base_branch(&bookmark.name, segments, default_branch)?;
            let title = generate_pr_title(&bookmark.name, segments)?;

            prs_to_create.push(PrToCreate {
                bookmark: (*bookmark).clone(),
                base_branch,
                title,
                draft: false,
            });
        }
    }

    Ok(SubmissionPlan {
        segments: segments.clone(),
        bookmarks_needing_push,
        prs_to_create,
        prs_to_update_base,
        prs_to_publish: Vec::new(),
        existing_prs,
        remote: remote.to_string(),
        default_branch: default_branch.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::NarrowedBookmarkSegment;

    fn make_bookmark(name: &str, has_remote: bool, is_synced: bool) -> Bookmark {
        Bookmark {
            name: name.to_string(),
            commit_id: format!("{name}_commit"),
            change_id: format!("{name}_change"),
            has_remote,
            is_synced,
        }
    }

    #[test]
    fn test_bookmark_needs_push() {
        // No remote tracking
        let bm1 = make_bookmark("feat-a", false, false);
        assert!(!bm1.has_remote || !bm1.is_synced);

        // Has remote but not synced
        let bm2 = make_bookmark("feat-b", true, false);
        assert!(!bm2.has_remote || !bm2.is_synced);

        // Fully synced - doesn't need push
        let bm3 = make_bookmark("feat-c", true, true);
        assert!(bm3.has_remote && bm3.is_synced);
    }

    #[test]
    fn test_pr_to_create_structure() {
        let pr_create = PrToCreate {
            bookmark: make_bookmark("feat-a", false, false),
            base_branch: "main".to_string(),
            title: "Add feature A".to_string(),
            draft: false,
        };

        assert_eq!(pr_create.bookmark.name, "feat-a");
        assert_eq!(pr_create.base_branch, "main");
        assert_eq!(pr_create.title, "Add feature A");
        assert!(!pr_create.draft);
    }

    #[test]
    fn test_submission_plan_structure() {
        let plan = SubmissionPlan {
            segments: vec![NarrowedBookmarkSegment {
                bookmark: make_bookmark("feat-a", false, false),
                changes: vec![],
            }],
            bookmarks_needing_push: vec![make_bookmark("feat-a", false, false)],
            prs_to_create: vec![PrToCreate {
                bookmark: make_bookmark("feat-a", false, false),
                base_branch: "main".to_string(),
                title: "Test".to_string(),
                draft: false,
            }],
            prs_to_update_base: vec![],
            prs_to_publish: vec![],
            existing_prs: HashMap::new(),
            remote: "origin".to_string(),
            default_branch: "main".to_string(),
        };

        assert_eq!(plan.segments.len(), 1);
        assert_eq!(plan.bookmarks_needing_push.len(), 1);
        assert_eq!(plan.prs_to_create.len(), 1);
        assert!(plan.prs_to_update_base.is_empty());
        assert!(plan.prs_to_publish.is_empty());
    }
}
