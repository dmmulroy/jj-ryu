//! Submit command - submit a bookmark stack as PRs

use crate::cli::CliProgress;
use jj_ryu::error::{Error, Result};
use jj_ryu::graph::build_change_graph;
use jj_ryu::platform::{create_platform_service, parse_repo_info};
use jj_ryu::repo::{select_remote, JjWorkspace};
use jj_ryu::submit::{analyze_submission, create_submission_plan, execute_submission};
use std::path::Path;

/// Run the submit command
pub async fn run_submit(
    path: &Path,
    bookmark: &str,
    remote: Option<&str>,
    dry_run: bool,
) -> Result<()> {
    // Open workspace
    let mut workspace = JjWorkspace::open(path)?;

    // Get remotes and select one
    let remotes = workspace.git_remotes()?;
    let remote_name = select_remote(&remotes, remote)?;

    // Detect platform from remote URL
    let remote_info = remotes
        .iter()
        .find(|r| r.name == remote_name)
        .ok_or_else(|| Error::RemoteNotFound(remote_name.clone()))?;

    let platform_config = parse_repo_info(&remote_info.url)?;

    // Create platform service
    let platform = create_platform_service(&platform_config).await?;

    // Build change graph
    let graph = build_change_graph(&workspace)?;

    if graph.bookmarks.is_empty() {
        println!("No bookmarks found in repository");
        return Ok(());
    }

    // Check if target bookmark exists
    if !graph.bookmarks.contains_key(bookmark) {
        return Err(Error::BookmarkNotFound(bookmark.to_string()));
    }

    // Analyze submission
    let analysis = analyze_submission(&graph, bookmark)?;

    println!(
        "Submitting {} bookmark{} in stack:",
        analysis.segments.len(),
        if analysis.segments.len() == 1 { "" } else { "s" }
    );
    // Display newest (leaf) first, oldest (closest to trunk) last
    for segment in analysis.segments.iter().rev() {
        let synced = if segment.bookmark.is_synced {
            " (synced)"
        } else {
            ""
        };
        println!("  - {}{}", segment.bookmark.name, synced);
    }
    println!();

    // Get default branch
    let default_branch = workspace.default_branch()?;

    // Create submission plan
    let plan = create_submission_plan(&analysis, platform.as_ref(), &remote_name, &default_branch)
        .await?;

    // Execute plan
    let progress = CliProgress::verbose();
    let result = execute_submission(&plan, &mut workspace, platform.as_ref(), &progress, dry_run)
        .await?;

    // Summary
    if !dry_run {
        println!();
        if result.success {
            println!("Successfully submitted {} bookmark{}",
                analysis.segments.len(),
                if analysis.segments.len() == 1 { "" } else { "s" }
            );

            if !result.created_prs.is_empty() {
                println!("Created {} PR{}",
                    result.created_prs.len(),
                    if result.created_prs.len() == 1 { "" } else { "s" }
                );
            }
        } else {
            eprintln!("Submission failed");
            for err in &result.errors {
                eprintln!("  {err}");
            }
        }
    }

    Ok(())
}
