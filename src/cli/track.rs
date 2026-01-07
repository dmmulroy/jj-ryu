//! `ryu track` command - explicit bookmark tracking

use crate::cli::style::{Stylize, check};
use anyhow::Result;
use chrono::Utc;
use dialoguer::MultiSelect;
use jj_ryu::graph::build_change_graph;
use jj_ryu::repo::JjWorkspace;
use jj_ryu::tracking::{TrackedBookmark, load_tracking, save_tracking};
use std::io::{self, IsTerminal};
use std::path::Path;

/// Options for the track command.
pub struct TrackOptions {
    /// Track all bookmarks in `trunk()`..@
    pub all: bool,
    /// Re-track already-tracked bookmarks (update remote)
    pub force: bool,
    /// Associate with specific remote
    pub remote: Option<String>,
}

/// Run the track command.
#[allow(clippy::too_many_lines)]
pub async fn run_track(path: &Path, bookmarks: &[String], options: TrackOptions) -> Result<()> {
    let workspace = JjWorkspace::open(path)?;
    let workspace_root = workspace.workspace_root().to_path_buf();

    // Build graph to get available bookmarks
    let graph = build_change_graph(&workspace)?;

    // Get bookmarks in the stack
    let available_bookmarks: Vec<&str> = graph
        .stack
        .as_ref()
        .map(|stack| {
            stack
                .segments
                .iter()
                .flat_map(|seg| seg.bookmarks.iter().map(|b| b.name.as_str()))
                .collect()
        })
        .unwrap_or_default();

    if available_bookmarks.is_empty() {
        eprintln!("{}", "No bookmarks found in trunk()..@".error());
        eprintln!(
            "{}",
            "Create bookmarks with 'jj bookmark create' first".muted()
        );
        return Ok(());
    }

    // Load existing tracking state
    let mut state = load_tracking(&workspace_root)?;

    // Determine which bookmarks to track
    let bookmarks_to_track: Vec<&str> = if options.all {
        // Track all bookmarks in stack
        available_bookmarks
            .iter()
            .filter(|&&name| options.force || !state.is_tracked(name))
            .copied()
            .collect()
    } else if bookmarks.is_empty() {
        // No bookmarks specified and not --all: interactive selection
        let untracked: Vec<&str> = available_bookmarks
            .iter()
            .filter(|&&name| !state.is_tracked(name))
            .copied()
            .collect();

        if untracked.is_empty() {
            eprintln!("{}", "All bookmarks already tracked".muted());
            return Ok(());
        }

        // Check if stdin is a terminal for interactive selection
        if io::stdin().is_terminal() {
            interactive_select(&untracked)?
        } else {
            // Non-interactive: show usage
            eprintln!("{}", "No bookmarks specified".error());
            eprintln!(
                "{}",
                "Usage: ryu track <bookmark>... or ryu track --all".muted()
            );
            eprintln!();
            eprintln!("Available bookmarks in trunk()..@:");
            for name in &available_bookmarks {
                let status = if state.is_tracked(name) {
                    format!(" {}", "(tracked)".muted())
                } else {
                    String::new()
                };
                eprintln!("  {}{}", name.accent(), status);
            }
            return Ok(());
        }
    } else {
        // Validate specified bookmarks exist in stack
        let mut to_track = Vec::new();
        for name in bookmarks {
            if !available_bookmarks.contains(&name.as_str()) {
                eprintln!(
                    "{}",
                    format!("Bookmark '{name}' not found in trunk()..@").error()
                );
                continue;
            }
            if state.is_tracked(name) && !options.force {
                eprintln!(
                    "{}",
                    format!("Bookmark '{name}' already tracked (use --force to re-track)").muted()
                );
                continue;
            }
            to_track.push(name.as_str());
        }
        to_track
    };

    if bookmarks_to_track.is_empty() {
        eprintln!("{}", "No bookmarks selected".muted());
        return Ok(());
    }

    // Track the bookmarks
    let mut tracked_names = Vec::new();
    for name in &bookmarks_to_track {
        // Get change_id for the bookmark
        let change_id = workspace
            .get_change_id(name)?
            .ok_or_else(|| anyhow::anyhow!("Bookmark '{name}' has no change_id"))?;

        let bookmark = TrackedBookmark {
            name: (*name).to_string(),
            change_id,
            remote: options.remote.clone(),
            tracked_at: Utc::now(),
        };

        // If force-tracking, remove existing entry first
        if options.force && state.is_tracked(name) {
            state.untrack(name);
        }

        // Track if not already tracked
        if !state.is_tracked(name) {
            state.track(bookmark);
            tracked_names.push(*name);
        }
    }

    // Save state
    save_tracking(&workspace_root, &state)?;

    // Print summary
    if tracked_names.len() == 1 {
        eprintln!("Tracked 1 bookmark:");
    } else {
        eprintln!("Tracked {} bookmarks:", tracked_names.len());
    }
    for name in &tracked_names {
        eprintln!("  {} {}", check(), name.accent());
    }

    Ok(())
}

/// Interactive bookmark selection using dialoguer.
fn interactive_select<'a>(bookmarks: &[&'a str]) -> Result<Vec<&'a str>> {
    let items: Vec<String> = bookmarks.iter().map(|&name| name.to_string()).collect();

    let selections = MultiSelect::new()
        .with_prompt("Select bookmarks to track (space to toggle, enter to confirm)")
        .items(&items)
        .interact()
        .map_err(|e| anyhow::anyhow!("Failed to read selection: {e}"))?;

    Ok(selections.into_iter().map(|i| bookmarks[i]).collect())
}
