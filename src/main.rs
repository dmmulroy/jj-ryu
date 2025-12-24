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
    /// Submit a bookmark stack as PRs
    Submit {
        /// Bookmark name to submit
        bookmark: String,

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
    },

    /// Sync all stacks with remote
    Sync {
        /// Dry run - show what would be done without making changes
        #[arg(long)]
        dry_run: bool,

        /// Preview plan and prompt for confirmation before executing
        #[arg(long, short = 'c')]
        confirm: bool,

        /// Only sync the stack containing this bookmark
        #[arg(long)]
        stack: Option<String>,

        /// Git remote to sync with
        #[arg(long)]
        remote: Option<String>,
    },

    /// Authentication management
    Auth {
        #[command(subcommand)]
        platform: AuthPlatform,
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
                &bookmark,
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
                },
            )
            .await?;
        }
        Some(Commands::Sync {
            dry_run,
            confirm,
            stack,
            remote,
        }) => {
            cli::run_sync(
                &path,
                remote.as_deref(),
                cli::SyncOptions {
                    dry_run,
                    confirm,
                    stack: stack.as_deref(),
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
    }

    Ok(())
}
