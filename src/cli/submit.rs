//! Submit command - submit a bookmark stack as PRs

use crate::cli::CliProgress;
use crate::cli::style::{CHECK, Stylize, arrow, bullet, cross};
use anstream::{eprintln, println};
use dialoguer::Confirm;
use jj_ryu::error::{Error, Result};
use jj_ryu::graph::build_change_graph;
use jj_ryu::platform::{PlatformService, create_platform_service, parse_repo_info};
use jj_ryu::repo::{JjWorkspace, select_remote};
use jj_ryu::submit::{
    ExecutionStep, SubmissionAnalysis, SubmissionPlan, analyze_submission, create_submission_plan,
    execute_submission, select_bookmark_for_segment,
};
use jj_ryu::tracking::{load_pr_cache, load_tracking, save_pr_cache};
use jj_ryu::types::{ChangeGraph, NarrowedBookmarkSegment};
use std::path::Path;

/// Scope of bookmark submission (mutually exclusive options)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SubmitScope {
    /// Default: submit from trunk to target bookmark
    #[default]
    Default,
    /// Submit only up to (and including) a specified bookmark
    Upto,
    /// Submit only the target bookmark (parent must have PR)
    Only,
    /// Include all descendants (upstack) in submission
    Stack,
}

impl std::fmt::Display for SubmitScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Default => Ok(()),
            Self::Upto => write!(f, " (--upto)"),
            Self::Only => write!(f, " (--only)"),
            Self::Stack => write!(f, " (--stack)"),
        }
    }
}

/// Options for the submit command
#[derive(Debug, Clone, Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct SubmitOptions<'a> {
    /// Dry run - show what would be done without making changes
    pub dry_run: bool,
    /// Preview plan and prompt for confirmation before executing
    pub confirm: bool,
    /// Scope of submission (default, upto, only, or stack)
    pub scope: SubmitScope,
    /// Bookmark name for --upto (only valid when scope == Upto)
    pub upto_bookmark: Option<&'a str>,
    /// Only update existing PRs, don't create new ones
    pub update_only: bool,
    /// Create new PRs as drafts
    pub draft: bool,
    /// Publish any draft PRs
    pub publish: bool,
    /// Interactively select which bookmarks to submit
    pub select: bool,
    /// Submit all bookmarks in `trunk()`..@ (ignore tracking)
    pub all: bool,
}

/// Run the submit command
#[allow(clippy::too_many_lines)]
pub async fn run_submit(
    path: &Path,
    bookmark: Option<&str>,
    remote: Option<&str>,
    options: SubmitOptions<'_>,
) -> Result<()> {
    // Validate conflicting options (scope conflicts handled by clap arg groups)
    if options.draft && options.publish {
        return Err(Error::InvalidArgument(
            "Cannot use --draft and --publish together".to_string(),
        ));
    }

    // Open workspace
    let mut workspace = JjWorkspace::open(path)?;
    let workspace_root = workspace.workspace_root().to_path_buf();

    // Load tracking state (unless --all bypasses tracking)
    let tracking = load_tracking(&workspace_root)?;
    let tracked_names: Vec<&str> = tracking.tracked_names().into_iter().collect();

    // If no bookmarks tracked and not --all, error
    if tracked_names.is_empty() && !options.all {
        return Err(Error::Tracking(
            "No bookmarks tracked. Run 'ryu track' first, or use 'ryu submit --all' to submit all bookmarks.".to_string()
        ));
    }

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

    // Build change graph from working copy
    let graph = build_change_graph(&workspace)?;

    // Check if we have a stack
    if graph.stack.is_none() {
        println!(
            "{}",
            "No bookmarks found between trunk and working copy.".muted()
        );
        println!(
            "{}",
            "Create a bookmark with: jj bookmark create <name>".muted()
        );
        return Ok(());
    }

    // If bookmark specified, verify it exists in stack
    if let Some(bm) = bookmark {
        if !graph.bookmarks.contains_key(bm) {
            return Err(Error::BookmarkNotFound(bm.to_string()));
        }
    }

    // Analyze submission based on options
    let mut analysis = build_analysis(&graph, bookmark, &options, platform.as_ref()).await?;

    // Filter to tracked bookmarks unless --all
    if !options.all && !tracked_names.is_empty() {
        analysis
            .segments
            .retain(|s| tracked_names.contains(&s.bookmark.name.as_str()));
        if analysis.segments.is_empty() {
            return Err(Error::Tracking(
                "No tracked bookmarks in submission scope. Use 'ryu track' to track bookmarks, or 'ryu submit --all'.".to_string()
            ));
        }
    }

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
            println!("{}", "No bookmarks selected, aborting".muted());
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
            println!("{}", "Aborted".muted());
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

    // Update PR cache with results
    if !options.dry_run && result.success {
        let mut pr_cache = load_pr_cache(&workspace_root).unwrap_or_default();
        for pr in result.created_prs.iter().chain(result.updated_prs.iter()) {
            pr_cache.upsert(&pr.head_ref, pr, &remote_name);
        }
        // Best effort - don't fail submit if cache write fails
        let _ = save_pr_cache(&workspace_root, &pr_cache);
    }

    // Summary
    if !options.dry_run {
        println!();
        if result.success {
            println!(
                "{} {} bookmark{}",
                format!("{CHECK} Successfully submitted").success(),
                analysis.segments.len().accent(),
                if analysis.segments.len() == 1 {
                    ""
                } else {
                    "s"
                }
            );

            if !result.created_prs.is_empty() {
                println!(
                    "Created {} PR{}",
                    result.created_prs.len().accent(),
                    if result.created_prs.len() == 1 {
                        ""
                    } else {
                        "s"
                    }
                );
            }
        } else {
            eprintln!("{} Submission failed", cross());
            for err in &result.errors {
                eprintln!("  {}", err.error());
            }
        }
    }

    Ok(())
}

/// Build submission analysis based on options
async fn build_analysis(
    graph: &ChangeGraph,
    bookmark: Option<&str>,
    options: &SubmitOptions<'_>,
    platform: &dyn PlatformService,
) -> Result<SubmissionAnalysis> {
    // Start with standard analysis (uses bookmark or leaf if None)
    let mut analysis = analyze_submission(graph, bookmark)?;
    debug_assert!(
        !analysis.segments.is_empty(),
        "analyze_submission returns Ok only if segments exist"
    );
    let target = analysis.target_bookmark.clone();

    match options.scope {
        SubmitScope::Default => {}

        SubmitScope::Upto => {
            // Handle --upto: truncate at specified bookmark
            let upto_bookmark = options.upto_bookmark.ok_or_else(|| {
                Error::InvalidArgument("--upto requires a bookmark name".to_string())
            })?;

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
                        "Bookmark '{upto_bookmark}' not found in stack"
                    )));
                }
            }
        }

        SubmitScope::Only => {
            // Handle --only: single bookmark submission
            let target_idx = analysis
                .segments
                .iter()
                .position(|s| s.bookmark.name == target);

            let target_idx = target_idx.ok_or_else(|| {
                Error::InvalidArgument(format!("Target bookmark '{target}' not found in analysis"))
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

        SubmitScope::Stack => {
            // Handle --stack (upstack): include all segments from target to leaf
            // With single-stack semantics, we can use graph.stack directly
            let stack = graph
                .stack
                .as_ref()
                .expect("stack existence checked before build_analysis");

            // Find target position in the full stack
            let target_idx = stack
                .segments
                .iter()
                .position(|s| s.bookmarks.iter().any(|b| b.name == target))
                .expect("target was set by analyze_submission");

            // Build narrowed segments from target to leaf (skip segments before target)
            analysis.segments = stack.segments[target_idx..]
                .iter()
                .map(|segment| NarrowedBookmarkSegment {
                    bookmark: select_bookmark_for_segment(segment, Some(&target)),
                    changes: segment.changes.clone(),
                })
                .collect();

            // Update target to reflect the new leaf
            if let Some(last) = analysis.segments.last() {
                analysis.target_bookmark.clone_from(&last.bookmark.name);
            }
        }
    }

    Ok(analysis)
}

/// Apply plan modifications based on options
fn apply_plan_options(plan: &mut SubmissionPlan, options: &SubmitOptions<'_>) {
    // Handle --update-only: remove PR creation steps and filter to existing PRs
    if options.update_only {
        plan.execution_steps.retain(|step| {
            match step {
                ExecutionStep::CreatePr(_) => false, // Remove all creates
                ExecutionStep::Push(bm) => plan.existing_prs.contains_key(&bm.name),
                _ => true,
            }
        });
    }

    // Handle --draft: mark new PRs as drafts (unless --publish is also set)
    // When both flags are present, --publish takes precedence and --draft is ignored
    if options.draft && !options.publish {
        for step in &mut plan.execution_steps {
            if let ExecutionStep::CreatePr(create) = step {
                create.draft = true;
            }
        }
    }

    // Handle --publish: publish existing draft PRs
    //
    // These steps are appended without constraint resolution because:
    // 1. They only operate on PRs that already exist (from previous runs)
    // 2. Publishing has no ordering dependencies with push/create/update operations
    if options.publish {
        let publish_steps: Vec<_> = plan
            .existing_prs
            .values()
            .filter(|pr| pr.is_draft)
            .map(|pr| ExecutionStep::PublishPr(pr.clone()))
            .collect();

        plan.execution_steps.extend(publish_steps);
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
            format!("{} {}", s.bookmark.name, status.muted())
        })
        .collect();

    let defaults: Vec<bool> = analysis.segments.iter().map(|_| true).collect();

    let selections = MultiSelect::new()
        .with_prompt("Select bookmarks to submit (space to toggle, enter to confirm)")
        .items(&items)
        .defaults(&defaults)
        .interact()
        .map_err(|e| Error::Internal(format!("Failed to read selection: {e}")))?;

    // Validate selection is contiguous (no gaps).
    // A contiguous selection has span == count: max - min + 1 == len
    if !selections.is_empty() {
        let min_idx = *selections
            .iter()
            .min()
            .expect("selections verified non-empty");
        let max_idx = *selections
            .iter()
            .max()
            .expect("selections verified non-empty");
        let span = max_idx - min_idx + 1;

        if span != selections.len() {
            // Find first gap for error message
            let gap_idx = (min_idx..=max_idx)
                .find(|i| !selections.contains(i))
                .expect("gap exists since span != len");
            return Err(Error::InvalidArgument(format!(
                "Cannot submit - selection has gap at '{}'. Stacked PRs must be contiguous.",
                analysis.segments[gap_idx].bookmark.name
            )));
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
    plan.execution_steps
        .retain(|step| selected.contains(&step.bookmark_name().to_string()));
}

/// Print submission summary
fn print_submission_summary(analysis: &SubmissionAnalysis, options: &SubmitOptions<'_>) {
    println!(
        "{} {} bookmark{}{}:",
        "Submitting".emphasis(),
        analysis.segments.len().accent(),
        if analysis.segments.len() == 1 {
            ""
        } else {
            "s"
        },
        options.scope.to_string().muted()
    );

    // Display newest (leaf) first, oldest (closest to trunk) last
    for segment in analysis.segments.iter().rev() {
        let synced = if segment.bookmark.is_synced {
            format!(" {}", "(synced)".muted())
        } else {
            String::new()
        };
        println!(
            "  {} {}{}",
            bullet(),
            segment.bookmark.name.accent(),
            synced
        );
    }
    println!();
}

/// Print plan preview for --confirm
fn print_plan_preview(plan: &SubmissionPlan) {
    println!("{}:", "Plan".emphasis());

    if plan.execution_steps.is_empty() {
        println!("  {}", "Nothing to do - already in sync".muted());
        println!();
        return;
    }

    println!("  {}:", "Steps".emphasis());
    for step in &plan.execution_steps {
        println!("    {} {}", arrow(), step);
    }

    println!();
}
