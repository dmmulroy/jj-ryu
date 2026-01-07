//! Sync command - sync current stack with remote

use crate::cli::CliProgress;
use crate::cli::style::{CHECK, Stylize, arrow, check, spinner_style};
use anstream::println;
use dialoguer::Confirm;
use indicatif::ProgressBar;
use jj_ryu::error::{Error, Result};
use jj_ryu::graph::build_change_graph;
use jj_ryu::platform::{create_platform_service, parse_repo_info};
use jj_ryu::repo::{JjWorkspace, select_remote};
use jj_ryu::submit::{
    SubmissionPlan, analyze_submission, create_submission_plan, execute_submission,
};
use jj_ryu::tracking::load_tracking;
use std::path::Path;
use std::time::Duration;

/// Options for the sync command
#[derive(Debug, Clone, Default)]
pub struct SyncOptions {
    /// Dry run - show what would be done without making changes
    pub dry_run: bool,
    /// Preview plan and prompt for confirmation before executing
    pub confirm: bool,
    /// Sync all bookmarks in `trunk()`..@ (ignore tracking)
    pub all: bool,
}

/// Run the sync command
#[allow(clippy::too_many_lines)]
pub async fn run_sync(path: &Path, remote: Option<&str>, options: SyncOptions) -> Result<()> {
    // Open workspace
    let mut workspace = JjWorkspace::open(path)?;
    let workspace_root = workspace.workspace_root().to_path_buf();

    // Load tracking state (unless --all bypasses tracking)
    let tracking = load_tracking(&workspace_root)?;
    let tracked_names: Vec<&str> = tracking.tracked_names().into_iter().collect();

    // If no bookmarks tracked and not --all, error
    if tracked_names.is_empty() && !options.all {
        return Err(Error::Tracking(
            "No bookmarks tracked. Run 'ryu track' first, or use 'ryu sync --all' to sync all bookmarks.".to_string()
        ));
    }

    // Get remotes and select one
    let remotes = workspace.git_remotes()?;
    let remote_name = select_remote(&remotes, remote)?;

    // Detect platform
    let remote_info = remotes
        .iter()
        .find(|r| r.name == remote_name)
        .ok_or_else(|| Error::RemoteNotFound(remote_name.clone()))?;

    let platform_config = parse_repo_info(&remote_info.url)?;

    // Create platform service
    let platform = create_platform_service(&platform_config).await?;

    // Fetch from remote with spinner
    if !options.dry_run {
        let spinner = ProgressBar::new_spinner();
        spinner.set_style(spinner_style());
        spinner.set_message(format!("Fetching from {}...", remote_name.emphasis()));
        spinner.enable_steady_tick(Duration::from_millis(80));

        workspace.git_fetch(&remote_name)?;

        spinner.finish_with_message(format!(
            "{} Fetched from {}",
            check(),
            remote_name.emphasis()
        ));
    }

    // Build change graph from working copy
    let graph = build_change_graph(&workspace)?;

    if graph.stack.is_none() {
        println!("{}", "No stack to sync".muted());
        println!(
            "{}",
            "Create bookmarks between trunk and working copy first.".muted()
        );
        return Ok(());
    }

    let default_branch = workspace.default_branch()?;
    let progress = CliProgress::compact();

    // Analyze and plan for the single stack
    let mut analysis = analyze_submission(&graph, None)?;

    // Filter to tracked bookmarks unless --all
    if !options.all && !tracked_names.is_empty() {
        analysis
            .segments
            .retain(|s| tracked_names.contains(&s.bookmark.name.as_str()));
        if analysis.segments.is_empty() {
            return Err(Error::Tracking(
                "No tracked bookmarks in stack. Use 'ryu track' to track bookmarks, or 'ryu sync --all'.".to_string()
            ));
        }
    }

    let plan =
        create_submission_plan(&analysis, platform.as_ref(), &remote_name, &default_branch).await?;

    // Show confirmation if requested
    if options.confirm && !options.dry_run {
        print_sync_preview(&plan);
        if !Confirm::new()
            .with_prompt("Proceed with sync?")
            .default(true)
            .interact()
            .map_err(|e| Error::Internal(format!("Failed to read confirmation: {e}")))?
        {
            println!("{}", "Aborted".muted());
            return Ok(());
        }
        println!();
    }

    // Execute
    println!(
        "{} {}",
        "Syncing stack:".emphasis(),
        analysis.target_bookmark.accent()
    );

    let result = execute_submission(
        &plan,
        &mut workspace,
        platform.as_ref(),
        &progress,
        options.dry_run,
    )
    .await?;

    // Summary
    println!();
    if options.dry_run {
        println!("{}", "Dry run complete".muted());
    } else {
        println!(
            "{} {} pushed, {} created, {} updated",
            format!("{CHECK} Sync complete:").success(),
            result.pushed_bookmarks.len().accent(),
            result.created_prs.len().accent(),
            result.updated_prs.len().accent()
        );
    }

    Ok(())
}

/// Print sync preview for --confirm
fn print_sync_preview(plan: &SubmissionPlan) {
    println!("{}:", "Sync plan".emphasis());
    println!();

    if plan.execution_steps.is_empty() {
        println!("  {}", "Already in sync".muted());
        println!();
        return;
    }

    println!("  {}:", "Steps".emphasis());
    for step in &plan.execution_steps {
        println!("    {} {}", arrow(), step);
    }

    println!();
}
