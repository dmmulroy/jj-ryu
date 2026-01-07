//! ryu - Stacked PRs for Jujutsu
//!
//! CLI binary for managing stacked pull requests with jj.

use anyhow::Result;
use clap::{Parser, Subcommand};
use jj_ryu::types::Platform;
use std::path::PathBuf;

mod cli;

#[derive(Parser)]
#[command(name = "ryu")]
#[command(about = "Stacked PRs for Jujutsu - GitHub & GitLab")]
#[command(version)]
struct Cli {
    /// Path to jj repository (defaults to current directory)
    #[arg(short, long, global = true)]
    path: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Submit current stack as PRs
    Submit {
        /// Bookmark to submit up to (defaults to leaf/top of stack)
        bookmark: Option<String>,

        /// Dry run - show what would be done without making changes
        #[arg(long)]
        dry_run: bool,

        /// Preview plan and prompt for confirmation before executing
        #[arg(long, short = 'c')]
        confirm: bool,

        /// Submit only up to (and including) this bookmark
        #[arg(long, group = "scope")]
        upto: Option<String>,

        /// Submit only this bookmark (parent must already have a PR)
        #[arg(long, group = "scope")]
        only: bool,

        /// Only update existing PRs, don't create new ones
        #[arg(long)]
        update_only: bool,

        /// Include all descendants (upstack) in submission
        #[arg(long, short = 's', group = "scope")]
        stack: bool,

        /// Create new PRs as drafts
        #[arg(long)]
        draft: bool,

        /// Publish any draft PRs
        #[arg(long)]
        publish: bool,

        /// Interactively select which bookmarks to submit
        #[arg(long, short = 'i')]
        select: bool,

        /// Git remote to push to
        #[arg(long)]
        remote: Option<String>,

        /// Submit all bookmarks in `trunk()`..@ (ignore tracking)
        #[arg(long, short)]
        all: bool,
    },

    /// Sync current stack with remote
    Sync {
        /// Dry run - show what would be done without making changes
        #[arg(long)]
        dry_run: bool,

        /// Preview plan and prompt for confirmation before executing
        #[arg(long, short = 'c')]
        confirm: bool,

        /// Git remote to sync with
        #[arg(long)]
        remote: Option<String>,

        /// Sync all bookmarks in `trunk()`..@ (ignore tracking)
        #[arg(long, short)]
        all: bool,
    },

    /// Authentication management
    Auth {
        #[command(subcommand)]
        platform: AuthPlatform,
    },

    /// Track bookmarks for submission
    Track {
        /// Bookmarks to track (shows available if omitted)
        bookmarks: Vec<String>,

        /// Track all bookmarks in `trunk()`..@
        #[arg(long, short)]
        all: bool,

        /// Re-track already-tracked bookmarks (update remote)
        #[arg(long, short)]
        force: bool,

        /// Associate with specific remote
        #[arg(long, short)]
        remote: Option<String>,
    },

    /// Stop tracking bookmarks
    Untrack {
        /// Bookmarks to untrack (shows tracked if omitted)
        bookmarks: Vec<String>,

        /// Untrack all tracked bookmarks
        #[arg(long, short)]
        all: bool,
    },
}

#[derive(Subcommand)]
enum AuthPlatform {
    /// GitHub authentication
    Github {
        #[command(subcommand)]
        action: AuthAction,
    },
    /// GitLab authentication
    Gitlab {
        #[command(subcommand)]
        action: AuthAction,
    },
}

#[derive(Subcommand)]
enum AuthAction {
    /// Test authentication
    Test,
    /// Show authentication setup instructions
    Setup,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let path = cli.path.unwrap_or_else(|| PathBuf::from("."));

    match cli.command {
        None => {
            // Default: interactive mode
            cli::run_analyze(&path).await?;
        }
        Some(Commands::Submit {
            bookmark,
            dry_run,
            confirm,
            upto,
            only,
            update_only,
            stack,
            draft,
            publish,
            select,
            remote,
            all,
        }) => {
            // Determine scope from mutually exclusive flags (enforced by clap arg groups)
            #[allow(clippy::option_if_let_else)]
            let (scope, upto_bookmark) = if let Some(ref upto_bm) = upto {
                (cli::SubmitScope::Upto, Some(upto_bm.as_str()))
            } else if only {
                (cli::SubmitScope::Only, None)
            } else if stack {
                (cli::SubmitScope::Stack, None)
            } else {
                (cli::SubmitScope::Default, None)
            };

            cli::run_submit(
                &path,
                bookmark.as_deref(),
                remote.as_deref(),
                cli::SubmitOptions {
                    dry_run,
                    confirm,
                    scope,
                    upto_bookmark,
                    update_only,
                    draft,
                    publish,
                    select,
                    all,
                },
            )
            .await?;
        }
        Some(Commands::Sync {
            dry_run,
            confirm,
            remote,
            all,
        }) => {
            cli::run_sync(
                &path,
                remote.as_deref(),
                cli::SyncOptions {
                    dry_run,
                    confirm,
                    all,
                },
            )
            .await?;
        }
        Some(Commands::Auth { platform }) => match platform {
            AuthPlatform::Github { action } => {
                let action_str = match action {
                    AuthAction::Test => "test",
                    AuthAction::Setup => "setup",
                };
                cli::run_auth(Platform::GitHub, action_str).await?;
            }
            AuthPlatform::Gitlab { action } => {
                let action_str = match action {
                    AuthAction::Test => "test",
                    AuthAction::Setup => "setup",
                };
                cli::run_auth(Platform::GitLab, action_str).await?;
            }
        },
        Some(Commands::Track {
            bookmarks,
            all,
            force,
            remote,
        }) => {
            cli::run_track(&path, &bookmarks, cli::TrackOptions { all, force, remote }).await?;
        }
        Some(Commands::Untrack { bookmarks, all }) => {
            cli::run_untrack(&path, &bookmarks, cli::UntrackOptions { all }).await?;
        }
    }

    Ok(())
}
