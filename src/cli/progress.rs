//! Shared CLI progress callback

use async_trait::async_trait;
use jj_ryu::error::Error;
use jj_ryu::submit::{Phase, ProgressCallback, PushStatus};
use jj_ryu::types::PullRequest;

/// CLI progress callback that prints to stdout
///
/// Two modes:
/// - verbose (submit): shows all phases, detailed messages
/// - compact (sync): inline status updates, indented for nested output
pub struct CliProgress {
    /// Verbose mode shows all phases and detailed output
    pub verbose: bool,
}

impl CliProgress {
    /// Create verbose progress (for submit command)
    pub fn verbose() -> Self {
        Self { verbose: true }
    }

    /// Create compact progress (for sync command)
    pub fn compact() -> Self {
        Self { verbose: false }
    }
}

#[async_trait]
impl ProgressCallback for CliProgress {
    async fn on_phase(&self, phase: Phase) {
        if self.verbose {
            match phase {
                Phase::Analyzing => println!("Analyzing..."),
                Phase::Planning => println!("Planning..."),
                Phase::Pushing => println!("Pushing bookmarks..."),
                Phase::CreatingPrs => println!("Creating PRs..."),
                Phase::UpdatingPrs => println!("Updating PRs..."),
                Phase::AddingComments => println!("Updating stack comments..."),
                Phase::Complete => println!("Done!"),
            }
        } else {
            match phase {
                Phase::Pushing => println!("  Pushing bookmarks..."),
                Phase::CreatingPrs => println!("  Creating PRs..."),
                Phase::UpdatingPrs => println!("  Updating PRs..."),
                Phase::AddingComments => println!("  Updating comments..."),
                _ => {}
            }
        }
    }

    async fn on_bookmark_push(&self, bookmark: &str, status: PushStatus) {
        if self.verbose {
            match status {
                PushStatus::Started => println!("  Pushing {bookmark}..."),
                PushStatus::Success => println!("  ✓ Pushed {bookmark}"),
                PushStatus::AlreadySynced => println!("  - {bookmark} already synced"),
                PushStatus::Failed(msg) => println!("  ✗ Failed to push {bookmark}: {msg}"),
            }
        } else {
            match status {
                PushStatus::Started => print!("    Pushing {bookmark}... "),
                PushStatus::Success => println!("done"),
                PushStatus::AlreadySynced => println!("already synced"),
                PushStatus::Failed(msg) => println!("failed: {msg}"),
            }
        }
    }

    async fn on_pr_created(&self, bookmark: &str, pr: &PullRequest) {
        if self.verbose {
            println!("  ✓ Created PR #{} for {}", pr.number, bookmark);
            println!("    {}", pr.html_url);
        } else {
            println!(
                "    Created PR #{} for {} ({})",
                pr.number, bookmark, pr.html_url
            );
        }
    }

    async fn on_pr_updated(&self, bookmark: &str, pr: &PullRequest) {
        if self.verbose {
            println!("  ✓ Updated PR #{} for {}", pr.number, bookmark);
        } else {
            println!("    Updated PR #{} for {}", pr.number, bookmark);
        }
    }

    async fn on_error(&self, error: &Error) {
        if self.verbose {
            eprintln!("Error: {error}");
        } else {
            eprintln!("    Error: {error}");
        }
    }

    async fn on_message(&self, message: &str) {
        if self.verbose {
            println!("{message}");
        } else {
            println!("  {message}");
        }
    }
}
