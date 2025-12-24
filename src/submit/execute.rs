//! Phase 3: Submission execution
//!
//! Executes the submission plan: push, create PRs, update bases, add comments.

use crate::error::{Error, Result};
use crate::platform::PlatformService;
use crate::repo::JjWorkspace;
use crate::submit::{Phase, ProgressCallback, PushStatus, SubmissionPlan};
use crate::types::PullRequest;
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Write;

/// Result of submission execution
#[derive(Debug, Clone)]
pub struct SubmissionResult {
    /// Whether execution succeeded
    pub success: bool,
    /// PRs that were created
    pub created_prs: Vec<PullRequest>,
    /// PRs that were updated (base changed)
    pub updated_prs: Vec<PullRequest>,
    /// Bookmarks that were pushed
    pub pushed_bookmarks: Vec<String>,
    /// Errors encountered (non-fatal)
    pub errors: Vec<String>,
}

/// Stack comment data embedded in PR comments
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StackCommentData {
    /// Schema version
    pub version: u8,
    /// PRs in the stack, ordered root to leaf
    pub stack: Vec<StackItem>,
}

/// A single item in the stack
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StackItem {
    /// Bookmark name for this PR
    pub bookmark_name: String,
    /// URL to the PR
    pub pr_url: String,
    /// PR number
    pub pr_number: u64,
}

/// Prefix for stack comment data
pub const COMMENT_DATA_PREFIX: &str = "<!--- JJ-RYU_STACK: ";
const COMMENT_DATA_PREFIX_OLD: &str = "<!--- JJ-STACK_INFO: ";
/// Postfix for stack comment data
pub const COMMENT_DATA_POSTFIX: &str = " --->";
/// Marker for the current PR in stack comments
pub const STACK_COMMENT_THIS_PR: &str = "👈";

/// Execute a submission plan
///
/// This performs the actual operations:
/// 1. Push bookmarks to remote
/// 2. Update PR bases
/// 3. Create new PRs
/// 4. Add/update stack comments
#[allow(clippy::too_many_lines)]
pub async fn execute_submission(
    plan: &SubmissionPlan,
    workspace: &mut JjWorkspace,
    platform: &dyn PlatformService,
    progress: &dyn ProgressCallback,
    dry_run: bool,
) -> Result<SubmissionResult> {
    let mut result = SubmissionResult {
        success: true,
        created_prs: Vec::new(),
        updated_prs: Vec::new(),
        pushed_bookmarks: Vec::new(),
        errors: Vec::new(),
    };

    if dry_run {
        progress
            .on_message("Dry run - no changes will be made")
            .await;
        report_dry_run(plan, progress).await;
        return Ok(result);
    }

    // Track all PRs (existing + created) for comment generation
    let mut bookmark_to_pr: HashMap<String, PullRequest> = plan.existing_prs.clone();

    // Phase: Pushing bookmarks
    progress.on_phase(Phase::Pushing).await;

    for bookmark in &plan.bookmarks_needing_push {
        progress
            .on_bookmark_push(&bookmark.name, PushStatus::Started)
            .await;

        match workspace.git_push(&bookmark.name, &plan.remote) {
            Ok(()) => {
                progress
                    .on_bookmark_push(&bookmark.name, PushStatus::Success)
                    .await;
                result.pushed_bookmarks.push(bookmark.name.clone());
            }
            Err(e) => {
                let msg = format!("Failed to push {}: {e}", bookmark.name);
                progress
                    .on_bookmark_push(&bookmark.name, PushStatus::Failed(msg.clone()))
                    .await;
                result.errors.push(msg);
                result.success = false;
                return Ok(result);
            }
        }
    }

    // Phase: Updating PR bases
    progress.on_phase(Phase::UpdatingPrs).await;

    for update in &plan.prs_to_update_base {
        progress
            .on_message(&format!(
                "Updating {} base: {} → {}",
                update.bookmark.name, update.current_base, update.expected_base
            ))
            .await;

        match platform
            .update_pr_base(update.pr.number, &update.expected_base)
            .await
        {
            Ok(updated_pr) => {
                progress
                    .on_pr_updated(&update.bookmark.name, &updated_pr)
                    .await;
                result.updated_prs.push(updated_pr.clone());
                bookmark_to_pr.insert(update.bookmark.name.clone(), updated_pr);
            }
            Err(e) => {
                let msg = format!("Failed to update PR base for {}: {e}", update.bookmark.name);
                progress.on_error(&Error::Platform(msg.clone())).await;
                result.errors.push(msg);
                result.success = false;
                return Ok(result);
            }
        }
    }

    // Phase: Creating PRs
    progress.on_phase(Phase::CreatingPrs).await;

    for pr_to_create in &plan.prs_to_create {
        let draft_str = if pr_to_create.draft { " [draft]" } else { "" };
        progress
            .on_message(&format!(
                "Creating PR for {} (base: {}){draft_str}",
                pr_to_create.bookmark.name, pr_to_create.base_branch
            ))
            .await;

        match platform
            .create_pr_with_options(
                &pr_to_create.bookmark.name,
                &pr_to_create.base_branch,
                &pr_to_create.title,
                pr_to_create.draft,
            )
            .await
        {
            Ok(pr) => {
                progress
                    .on_pr_created(&pr_to_create.bookmark.name, &pr)
                    .await;
                result.created_prs.push(pr.clone());
                bookmark_to_pr.insert(pr_to_create.bookmark.name.clone(), pr);
            }
            Err(e) => {
                let msg = format!(
                    "Failed to create PR for {}: {e}",
                    pr_to_create.bookmark.name
                );
                progress.on_error(&Error::Platform(msg.clone())).await;
                result.errors.push(msg);
                result.success = false;
                return Ok(result);
            }
        }
    }

    // Phase: Publishing draft PRs
    if !plan.prs_to_publish.is_empty() {
        progress.on_phase(Phase::PublishingPrs).await;

        for pr in &plan.prs_to_publish {
            progress
                .on_message(&format!("Publishing PR #{} ({})", pr.number, pr.head_ref))
                .await;

            match platform.publish_pr(pr.number).await {
                Ok(updated_pr) => {
                    result.updated_prs.push(updated_pr.clone());
                    bookmark_to_pr.insert(pr.head_ref.clone(), updated_pr);
                }
                Err(e) => {
                    let msg = format!("Failed to publish PR #{}: {e}", pr.number);
                    progress.on_error(&Error::Platform(msg.clone())).await;
                    result.errors.push(msg);
                    // Don't fail the whole submission for publish errors
                }
            }
        }
    }

    // Phase: Adding stack comments
    progress.on_phase(Phase::AddingComments).await;

    if !bookmark_to_pr.is_empty() {
        let stack_data = build_stack_comment_data(plan, &bookmark_to_pr);

        for (idx, item) in stack_data.stack.iter().enumerate() {
            if let Err(e) =
                create_or_update_stack_comment(platform, &stack_data, idx, item.pr_number).await
            {
                let msg = format!(
                    "Failed to update stack comment for {}: {e}",
                    item.bookmark_name
                );
                progress.on_error(&Error::Platform(msg.clone())).await;
                result.errors.push(msg);
                // Don't fail the whole submission for comment errors
            }
        }
    }

    progress.on_phase(Phase::Complete).await;

    Ok(result)
}

/// Report what would be done in a dry run
async fn report_dry_run(plan: &SubmissionPlan, progress: &dyn ProgressCallback) {
    if !plan.bookmarks_needing_push.is_empty() {
        progress.on_message("Would push:").await;
        for bm in &plan.bookmarks_needing_push {
            progress
                .on_message(&format!("  - {} to {}", bm.name, plan.remote))
                .await;
        }
    }

    if !plan.prs_to_update_base.is_empty() {
        progress.on_message("Would update PR bases:").await;
        for update in &plan.prs_to_update_base {
            progress
                .on_message(&format!(
                    "  - {} (PR #{}) {} → {}",
                    update.bookmark.name,
                    update.pr.number,
                    update.current_base,
                    update.expected_base
                ))
                .await;
        }
    }

    if !plan.prs_to_create.is_empty() {
        progress.on_message("Would create PRs:").await;
        for pr in &plan.prs_to_create {
            progress
                .on_message(&format!(
                    "  - {} → {} ({})",
                    pr.bookmark.name, pr.base_branch, pr.title
                ))
                .await;
        }
    }

    if plan.bookmarks_needing_push.is_empty()
        && plan.prs_to_update_base.is_empty()
        && plan.prs_to_create.is_empty()
    {
        progress.on_message("Nothing to do - already in sync").await;
    }
}

/// Build stack comment data from the plan and PRs
#[allow(clippy::implicit_hasher)]
pub fn build_stack_comment_data(
    plan: &SubmissionPlan,
    bookmark_to_pr: &HashMap<String, PullRequest>,
) -> StackCommentData {
    let stack: Vec<StackItem> = plan
        .segments
        .iter()
        .filter_map(|seg| {
            bookmark_to_pr.get(&seg.bookmark.name).map(|pr| StackItem {
                bookmark_name: seg.bookmark.name.clone(),
                pr_url: pr.html_url.clone(),
                pr_number: pr.number,
            })
        })
        .collect();

    StackCommentData { version: 0, stack }
}

/// Format the stack comment body for a PR
pub fn format_stack_comment(data: &StackCommentData, current_idx: usize) -> Result<String> {
    let encoded_data = BASE64.encode(
        serde_json::to_string(data)
            .map_err(|e| Error::Internal(format!("Failed to serialize stack data: {e}")))?,
    );

    let mut body = format!("{COMMENT_DATA_PREFIX}{encoded_data}{COMMENT_DATA_POSTFIX}\n");

    // Reverse order: newest/leaf at top, oldest at bottom
    // Use plain #X format so GitHub auto-links with status indicators
    let reversed_idx = data.stack.len() - 1 - current_idx;
    for (i, item) in data.stack.iter().rev().enumerate() {
        if i == reversed_idx {
            let _ = writeln!(body, "* **#{} {STACK_COMMENT_THIS_PR}**", item.pr_number);
        } else {
            let _ = writeln!(body, "* #{}", item.pr_number);
        }
    }

    let _ = write!(
        body,
        "\n---\nThis stack of pull requests is managed by [jj-ryu](https://github.com/dmmulroy/jj-ryu)."
    );

    Ok(body)
}

/// Create or update the stack comment on a PR
async fn create_or_update_stack_comment(
    platform: &dyn PlatformService,
    data: &StackCommentData,
    current_idx: usize,
    pr_number: u64,
) -> Result<()> {
    let body = format_stack_comment(data, current_idx)?;

    // Find existing comment by looking for our data prefix (check both old and new)
    let comments = platform.list_pr_comments(pr_number).await?;
    let existing = comments
        .iter()
        .find(|c| c.body.contains(COMMENT_DATA_PREFIX) || c.body.contains(COMMENT_DATA_PREFIX_OLD));

    if let Some(comment) = existing {
        platform
            .update_pr_comment(pr_number, comment.id, &body)
            .await?;
    } else {
        platform.create_pr_comment(pr_number, &body).await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Bookmark;

    fn make_pr(number: u64, bookmark: &str) -> PullRequest {
        PullRequest {
            number,
            html_url: format!("https://github.com/test/test/pull/{number}"),
            base_ref: "main".to_string(),
            head_ref: bookmark.to_string(),
            title: format!("PR for {bookmark}"),
            node_id: Some(format!("PR_node_{number}")),
            is_draft: false,
        }
    }

    fn make_bookmark(name: &str) -> Bookmark {
        Bookmark {
            name: name.to_string(),
            commit_id: format!("{name}_commit"),
            change_id: format!("{name}_change"),
            has_remote: false,
            is_synced: false,
        }
    }

    #[test]
    fn test_build_stack_comment_data() {
        use crate::types::NarrowedBookmarkSegment;

        let plan = SubmissionPlan {
            segments: vec![
                NarrowedBookmarkSegment {
                    bookmark: make_bookmark("feat-a"),
                    changes: vec![],
                },
                NarrowedBookmarkSegment {
                    bookmark: make_bookmark("feat-b"),
                    changes: vec![],
                },
            ],
            bookmarks_needing_push: vec![],
            prs_to_create: vec![],
            prs_to_update_base: vec![],
            prs_to_publish: vec![],
            existing_prs: HashMap::new(),
            remote: "origin".to_string(),
            default_branch: "main".to_string(),
        };

        let mut bookmark_to_pr = HashMap::new();
        bookmark_to_pr.insert("feat-a".to_string(), make_pr(1, "feat-a"));
        bookmark_to_pr.insert("feat-b".to_string(), make_pr(2, "feat-b"));

        let data = build_stack_comment_data(&plan, &bookmark_to_pr);

        assert_eq!(data.version, 0);
        assert_eq!(data.stack.len(), 2);
        assert_eq!(data.stack[0].bookmark_name, "feat-a");
        assert_eq!(data.stack[0].pr_number, 1);
        assert_eq!(data.stack[1].bookmark_name, "feat-b");
        assert_eq!(data.stack[1].pr_number, 2);
    }

    #[test]
    fn test_submission_result_default() {
        let result = SubmissionResult {
            success: true,
            created_prs: vec![],
            updated_prs: vec![],
            pushed_bookmarks: vec![],
            errors: vec![],
        };

        assert!(result.success);
        assert!(result.created_prs.is_empty());
        assert!(result.updated_prs.is_empty());
    }
}
