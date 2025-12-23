//! Progress callback trait for interface-agnostic updates
//!
//! This trait allows different interfaces (CLI, web server, etc.) to receive
//! progress updates during submission operations.

use crate::error::Error;
use crate::types::PullRequest;
use async_trait::async_trait;

/// Submission phase
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Analyzing the change graph
    Analyzing,
    /// Planning what to submit
    Planning,
    /// Pushing bookmarks to remote
    Pushing,
    /// Creating new PRs
    CreatingPrs,
    /// Updating existing PR base branches
    UpdatingPrs,
    /// Publishing draft PRs
    PublishingPrs,
    /// Adding/updating stack comments
    AddingComments,
    /// Submission complete
    Complete,
}

/// Push operation status
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushStatus {
    /// Push started
    Started,
    /// Push succeeded
    Success,
    /// Bookmark already synced with remote
    AlreadySynced,
    /// Push failed with error message
    Failed(String),
}

/// Progress callback trait
///
/// Implement this trait to receive progress updates during submission.
/// - CLI implementations can print to terminal
/// - Web servers can send SSE or WebSocket messages
#[async_trait]
pub trait ProgressCallback: Send + Sync {
    /// Called when entering a new phase
    async fn on_phase(&self, phase: Phase);

    /// Called when a bookmark is being pushed
    async fn on_bookmark_push(&self, bookmark: &str, status: PushStatus);

    /// Called when a PR is created
    async fn on_pr_created(&self, bookmark: &str, pr: &PullRequest);

    /// Called when a PR is updated
    async fn on_pr_updated(&self, bookmark: &str, pr: &PullRequest);

    /// Called when an error occurs (non-fatal)
    async fn on_error(&self, error: &Error);

    /// Called with a general status message
    async fn on_message(&self, message: &str);
}

/// No-op progress callback for testing or when progress isn't needed
pub struct NoopProgress;

#[async_trait]
impl ProgressCallback for NoopProgress {
    async fn on_phase(&self, _phase: Phase) {}
    async fn on_bookmark_push(&self, _bookmark: &str, _status: PushStatus) {}
    async fn on_pr_created(&self, _bookmark: &str, _pr: &PullRequest) {}
    async fn on_pr_updated(&self, _bookmark: &str, _pr: &PullRequest) {}
    async fn on_error(&self, _error: &Error) {}
    async fn on_message(&self, _message: &str) {}
}
