//! `ryu untrack` command - remove bookmarks from tracking

use crate::cli::style::{Stylize, check};
use anyhow::Result;
use dialoguer::MultiSelect;
use jj_ryu::repo::JjWorkspace;
use jj_ryu::tracking::{load_pr_cache, load_tracking, save_tracking};
use std::io::{self, IsTerminal};
use std::path::Path;

/// Options for the untrack command.
pub struct UntrackOptions {
    /// Untrack all tracked bookmarks
    pub all: bool,
}

/// Run the untrack command.
pub async fn run_untrack(path: &Path, bookmarks: &[String], options: UntrackOptions) -> Result<()> {
    let workspace = JjWorkspace::open(path)?;
    let workspace_root = workspace.workspace_root().to_path_buf();

    // Load existing tracking state
    let mut state = load_tracking(&workspace_root)?;

    if state.bookmarks.is_empty() {
        eprintln!("{}", "No bookmarks currently tracked".muted());
        return Ok(());
    }

    // Load PR cache for notes about open PRs
    let pr_cache = load_pr_cache(&workspace_root)?;

    // Determine which bookmarks to untrack
    let bookmarks_to_untrack: Vec<String> = if options.all {
        // Untrack all
        state
            .tracked_names()
            .into_iter()
            .map(String::from)
            .collect()
    } else if bookmarks.is_empty() {
        // Interactive selection
        let tracked: Vec<String> = state
            .tracked_names()
            .into_iter()
            .map(String::from)
            .collect();

        if io::stdin().is_terminal() {
            interactive_select(&tracked)?
        } else {
            eprintln!("{}", "No bookmarks specified".error());
            eprintln!(
                "{}",
                "Usage: ryu untrack <bookmark>... or ryu untrack --all".muted()
            );
            eprintln!();
            eprintln!("Currently tracked bookmarks:");
            for name in &tracked {
                let pr_note = pr_cache
                    .get(name)
                    .map(|p| format!(" {}", format!("(PR #{})", p.number).muted()))
                    .unwrap_or_default();
                eprintln!("  {}{}", name.accent(), pr_note);
            }
            return Ok(());
        }
    } else {
        // Validate specified bookmarks are tracked
        let mut to_untrack = Vec::new();
        for name in bookmarks {
            if !state.is_tracked(name) {
                eprintln!("{}", format!("Bookmark '{name}' is not tracked").warn());
                continue;
            }
            to_untrack.push(name.clone());
        }
        to_untrack
    };

    if bookmarks_to_untrack.is_empty() {
        eprintln!("{}", "No bookmarks selected".muted());
        return Ok(());
    }

    // Untrack the bookmarks
    let mut untracked_names = Vec::new();
    let mut pr_notes = Vec::new();
    for name in &bookmarks_to_untrack {
        if state.untrack(name) {
            untracked_names.push(name.clone());
            // Note any open PRs
            if let Some(cached) = pr_cache.get(name) {
                pr_notes.push(format!("PR #{} remains open", cached.number));
            }
        }
    }

    // Save state
    save_tracking(&workspace_root, &state)?;

    // Print summary
    if untracked_names.len() == 1 {
        eprintln!("Untracked 1 bookmark:");
    } else {
        eprintln!("Untracked {} bookmarks:", untracked_names.len());
    }
    for name in &untracked_names {
        eprintln!("  {} {}", check(), name.accent());
    }

    // Show PR notes
    if !pr_notes.is_empty() {
        eprintln!();
        for note in &pr_notes {
            eprintln!(
                "{}",
                format!("Note: {note}. Close manually if needed.").muted()
            );
        }
    }

    Ok(())
}

/// Interactive bookmark selection using dialoguer.
fn interactive_select(bookmarks: &[String]) -> Result<Vec<String>> {
    let selections = MultiSelect::new()
        .with_prompt("Select bookmarks to untrack (space to toggle, enter to confirm)")
        .items(bookmarks)
        .interact()
        .map_err(|e| anyhow::anyhow!("Failed to read selection: {e}"))?;

    Ok(selections
        .into_iter()
        .map(|i| bookmarks[i].clone())
        .collect())
}
