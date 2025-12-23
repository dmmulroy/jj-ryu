//! Submit command - submit a bookmark stack as PRs

use crate::cli::CliProgress;
use dialoguer::Confirm;
use jj_ryu::error::{Error, Result};
use jj_ryu::graph::build_change_graph;
use jj_ryu::platform::{PlatformService, create_platform_service, parse_repo_info};
use jj_ryu::repo::{JjWorkspace, select_remote};
use jj_ryu::submit::{
    SubmissionAnalysis, SubmissionPlan, analyze_submission, create_submission_plan,
    execute_submission,
};
use jj_ryu::types::ChangeGraph;
use std::path::Path;

/// Options for the submit command
#[derive(Debug, Clone, Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct SubmitOptions<'a> {
    /// Dry run - show what would be done without making changes
    pub dry_run: bool,
    /// Preview plan and prompt for confirmation before executing
    pub confirm: bool,
    /// Submit only up to (and including) this bookmark
    pub upto: Option<&'a str>,
    /// Submit only this bookmark (parent must already have a PR)
    pub only: bool,
    /// Only update existing PRs, don't create new ones
    pub update_only: bool,
    /// Include all descendants (upstack) in submission
    pub stack: bool,
    /// Create new PRs as drafts
    pub draft: bool,
    /// Publish any draft PRs
    pub publish: bool,
    /// Interactively select which bookmarks to submit
    pub select: bool,
}

/// Run the submit command
pub async fn run_submit(
    path: &Path,
    bookmark: &str,
    remote: Option<&str>,
    options: SubmitOptions<'_>,
) -> Result<()> {
    // Validate conflicting options
    if options.only && options.upto.is_some() {
        return Err(Error::InvalidArgument(
            "Cannot use --only and --upto together".to_string(),
        ));
    }
    if options.draft && options.publish {
        return Err(Error::InvalidArgument(
            "Cannot use --draft and --publish together".to_string(),
        ));
    }

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

    // Analyze submission based on options
    let analysis = build_analysis(&graph, bookmark, &options, platform.as_ref()).await?;

    // Display what will be submitted
    print_submission_summary(&analysis, &options);

    // Get default branch
    let default_branch = workspace.default_branch()?;

    // Create submission plan
    let mut plan =
        create_submission_plan(&analysis, platform.as_ref(), &remote_name, &default_branch).await?;

    // Apply plan modifications based on options
    apply_plan_options(&mut plan, &options);

    // Handle interactive selection
    if options.select {
        let selected = interactive_select(&analysis)?;
        if selected.is_empty() {
            println!("No bookmarks selected, aborting");
            return Ok(());
        }
        filter_plan_to_selection(&mut plan, &selected);
    }

    // Show confirmation if requested
    if options.confirm && !options.dry_run {
        print_plan_preview(&plan);
        if !Confirm::new()
            .with_prompt("Proceed with submission?")
            .default(true)
            .interact()
            .map_err(|e| Error::Internal(format!("Failed to read confirmation: {e}")))?
        {
            println!("Aborted");
            return Ok(());
        }
        println!();
    }

    // Execute plan
    let progress = CliProgress::verbose();
    let result = execute_submission(
        &plan,
        &mut workspace,
        platform.as_ref(),
        &progress,
        options.dry_run,
    )
    .await?;

    // Summary
    if !options.dry_run {
        println!();
        if result.success {
            println!(
                "Successfully submitted {} bookmark{}",
                analysis.segments.len(),
                if analysis.segments.len() == 1 {
                    ""
                } else {
                    "s"
                }
            );

            if !result.created_prs.is_empty() {
                println!(
                    "Created {} PR{}",
                    result.created_prs.len(),
                    if result.created_prs.len() == 1 {
                        ""
                    } else {
                        "s"
                    }
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

/// Build submission analysis based on options
async fn build_analysis(
    graph: &ChangeGraph,
    bookmark: &str,
    options: &SubmitOptions<'_>,
    platform: &dyn PlatformService,
) -> Result<SubmissionAnalysis> {
    // Start with standard analysis
    let mut analysis = analyze_submission(graph, bookmark)?;

    // Handle --upto: truncate at specified bookmark
    if let Some(upto_bookmark) = options.upto {
        // Validate upto bookmark exists in the analysis
        let upto_idx = analysis
            .segments
            .iter()
            .position(|s| s.bookmark.name == upto_bookmark);

        match upto_idx {
            Some(idx) => {
                analysis.segments.truncate(idx + 1);
                analysis.target_bookmark = upto_bookmark.to_string();
            }
            None => {
                return Err(Error::InvalidArgument(format!(
                    "Bookmark '{upto_bookmark}' not found in stack ancestors of '{bookmark}'"
                )));
            }
        }
    }

    // Handle --only: single bookmark submission
    if options.only {
        // Find the target segment
        let target_idx = analysis
            .segments
            .iter()
            .position(|s| s.bookmark.name == bookmark);

        let target_idx = target_idx.ok_or_else(|| {
            Error::Internal(format!(
                "Target bookmark '{bookmark}' not found in analysis"
            ))
        })?;

        // If not the first segment, verify parent has a PR
        if target_idx > 0 {
            let parent_bookmark = &analysis.segments[target_idx - 1].bookmark.name;
            let parent_pr = platform.find_existing_pr(parent_bookmark).await?;

            if parent_pr.is_none() {
                return Err(Error::InvalidArgument(format!(
                    "Cannot use --only: parent bookmark '{parent_bookmark}' has no PR. Use --upto instead."
                )));
            }
        }

        // Keep only the target segment
        analysis.segments = vec![analysis.segments.remove(target_idx)];
    }

    // Handle --stack (upstack): include descendants
    if options.stack {
        let descendants = find_all_descendants(graph, bookmark);
        for descendant_name in descendants {
            // Get analysis for each descendant and merge segments
            if let Ok(desc_analysis) = analyze_submission(graph, &descendant_name) {
                // Add segments that aren't already in our analysis
                for segment in desc_analysis.segments {
                    if !analysis
                        .segments
                        .iter()
                        .any(|s| s.bookmark.name == segment.bookmark.name)
                    {
                        analysis.segments.push(segment);
                    }
                }
            }
        }
    }

    Ok(analysis)
}

/// Find all descendant bookmarks (across all branching stacks)
fn find_all_descendants(graph: &ChangeGraph, bookmark: &str) -> Vec<String> {
    let mut descendants = Vec::new();

    // Get the change_id for this bookmark
    let Some(bookmark_change_id) = graph.bookmark_to_change_id.get(bookmark) else {
        return descendants;
    };

    // For each stack, check if our bookmark appears in the path
    for stack in &graph.stacks {
        let mut found_bookmark = false;
        for segment in &stack.segments {
            // Check if any bookmark in this segment matches
            if segment
                .bookmarks
                .iter()
                .any(|b| graph.bookmark_to_change_id.get(&b.name) == Some(bookmark_change_id))
            {
                found_bookmark = true;
                continue; // Skip the bookmark itself
            }

            // After finding our bookmark, all subsequent bookmarks are descendants
            if found_bookmark {
                for b in &segment.bookmarks {
                    if !descendants.contains(&b.name) && b.name != bookmark {
                        descendants.push(b.name.clone());
                    }
                }
            }
        }
    }

    descendants
}

/// Apply plan modifications based on options
fn apply_plan_options(plan: &mut SubmissionPlan, options: &SubmitOptions<'_>) {
    // Handle --update-only: remove PR creation
    if options.update_only {
        plan.prs_to_create.clear();

        // Also filter bookmarks_needing_push to only those with existing PRs
        plan.bookmarks_needing_push
            .retain(|b| plan.existing_prs.contains_key(&b.name));
    }

    // Handle --draft: mark new PRs as drafts
    if options.draft {
        for pr_to_create in &mut plan.prs_to_create {
            pr_to_create.draft = true;
        }
    }

    // Handle --publish: queue existing draft PRs for publishing
    if options.publish {
        for pr in plan.existing_prs.values() {
            if pr.is_draft {
                plan.prs_to_publish.push(pr.clone());
            }
            // Non-draft PRs silently skipped (matches Graphite behavior)
        }
    }
}

/// Interactive bookmark selection using dialoguer
fn interactive_select(analysis: &SubmissionAnalysis) -> Result<Vec<String>> {
    use dialoguer::MultiSelect;

    let items: Vec<String> = analysis
        .segments
        .iter()
        .map(|s| {
            let status = if s.bookmark.is_synced {
                "(synced)"
            } else if s.bookmark.has_remote {
                "(needs push)"
            } else {
                "(new)"
            };
            format!("{} {}", s.bookmark.name, status)
        })
        .collect();

    let defaults: Vec<bool> = analysis.segments.iter().map(|_| true).collect();

    let selections = MultiSelect::new()
        .with_prompt("Select bookmarks to submit (space to toggle, enter to confirm)")
        .items(&items)
        .defaults(&defaults)
        .interact()
        .map_err(|e| Error::Internal(format!("Failed to read selection: {e}")))?;

    // Validate selection is contiguous (no gaps)
    if !selections.is_empty() {
        let min_idx = *selections.iter().min().unwrap();
        let max_idx = *selections.iter().max().unwrap();

        for i in min_idx..=max_idx {
            if !selections.contains(&i) {
                return Err(Error::InvalidArgument(format!(
                    "Cannot submit - selection has gap at '{}'. Stacked PRs must be contiguous.",
                    analysis.segments[i].bookmark.name
                )));
            }
        }
    }

    Ok(selections
        .iter()
        .map(|&i| analysis.segments[i].bookmark.name.clone())
        .collect())
}

/// Filter plan to only include selected bookmarks
fn filter_plan_to_selection(plan: &mut SubmissionPlan, selected: &[String]) {
    plan.segments
        .retain(|s| selected.contains(&s.bookmark.name));
    plan.bookmarks_needing_push
        .retain(|b| selected.contains(&b.name));
    plan.prs_to_create
        .retain(|p| selected.contains(&p.bookmark.name));
    plan.prs_to_update_base
        .retain(|p| selected.contains(&p.bookmark.name));
}

/// Print submission summary
fn print_submission_summary(analysis: &SubmissionAnalysis, options: &SubmitOptions<'_>) {
    let mode = if options.only {
        " (--only)"
    } else if options.upto.is_some() {
        " (--upto)"
    } else if options.stack {
        " (--stack)"
    } else {
        ""
    };

    println!(
        "Submitting {} bookmark{}{} in stack:",
        analysis.segments.len(),
        if analysis.segments.len() == 1 {
            ""
        } else {
            "s"
        },
        mode
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
}

/// Print plan preview for --confirm
fn print_plan_preview(plan: &SubmissionPlan) {
    println!("Plan:");

    if !plan.bookmarks_needing_push.is_empty() {
        println!("  Push:");
        for bm in &plan.bookmarks_needing_push {
            println!("    - {} → {}", bm.name, plan.remote);
        }
    }

    if !plan.prs_to_update_base.is_empty() {
        println!("  Update PR bases:");
        for update in &plan.prs_to_update_base {
            println!(
                "    - {} (PR #{}) {} → {}",
                update.bookmark.name, update.pr.number, update.current_base, update.expected_base
            );
        }
    }

    if !plan.prs_to_create.is_empty() {
        println!("  Create PRs:");
        for pr in &plan.prs_to_create {
            let draft_str = if pr.draft { " [draft]" } else { "" };
            println!(
                "    - {} → {} ({}){draft_str}",
                pr.bookmark.name, pr.base_branch, pr.title
            );
        }
    }

    if !plan.prs_to_publish.is_empty() {
        println!("  Publish PRs:");
        for pr in &plan.prs_to_publish {
            println!("    - {} (PR #{})", pr.head_ref, pr.number);
        }
    }

    if plan.bookmarks_needing_push.is_empty()
        && plan.prs_to_update_base.is_empty()
        && plan.prs_to_create.is_empty()
        && plan.prs_to_publish.is_empty()
    {
        println!("  Nothing to do - already in sync");
    }

    println!();
}
