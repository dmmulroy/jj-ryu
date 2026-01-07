//! Bookmark tracking for explicit submission management.
//!
//! This module provides persistence for tracking which bookmarks should be
//! submitted to the remote platform. It stores metadata in `.jj/repo/ryu/`.

mod pr_cache;
mod storage;

pub use pr_cache::{
    CachedPr, PR_CACHE_VERSION, PrCache, load_pr_cache, pr_cache_path, save_pr_cache,
};
pub use storage::{load_tracking, save_tracking, tracking_path};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Current version of the tracking file format.
pub const TRACKING_VERSION: u32 = 1;

/// A bookmark that has been explicitly tracked for submission.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrackedBookmark {
    /// Bookmark name (e.g., "feat-auth").
    pub name: String,
    /// jj change ID for rename detection.
    pub change_id: String,
    /// Optional remote to submit to (defaults to auto-detect).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
    /// When this bookmark was tracked.
    pub tracked_at: DateTime<Utc>,
}

impl TrackedBookmark {
    /// Create a new tracked bookmark.
    pub fn new(name: String, change_id: String) -> Self {
        Self {
            name,
            change_id,
            remote: None,
            tracked_at: Utc::now(),
        }
    }

    /// Create a new tracked bookmark with a specific remote.
    pub fn with_remote(name: String, change_id: String, remote: String) -> Self {
        Self {
            name,
            change_id,
            remote: Some(remote),
            tracked_at: Utc::now(),
        }
    }
}

/// Persistent state of tracked bookmarks.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TrackingState {
    /// File format version.
    pub version: u32,
    /// List of tracked bookmarks.
    #[serde(default)]
    pub bookmarks: Vec<TrackedBookmark>,
}

impl TrackingState {
    /// Create a new empty tracking state.
    pub const fn new() -> Self {
        Self {
            version: TRACKING_VERSION,
            bookmarks: Vec::new(),
        }
    }

    /// Check if a bookmark is tracked.
    pub fn is_tracked(&self, name: &str) -> bool {
        self.bookmarks.iter().any(|b| b.name == name)
    }

    /// Get a tracked bookmark by name.
    pub fn get(&self, name: &str) -> Option<&TrackedBookmark> {
        self.bookmarks.iter().find(|b| b.name == name)
    }

    /// Add a bookmark to tracking (no-op if already tracked).
    pub fn track(&mut self, bookmark: TrackedBookmark) {
        if !self.is_tracked(&bookmark.name) {
            self.bookmarks.push(bookmark);
        }
    }

    /// Remove a bookmark from tracking. Returns true if it was removed.
    pub fn untrack(&mut self, name: &str) -> bool {
        let len_before = self.bookmarks.len();
        self.bookmarks.retain(|b| b.name != name);
        self.bookmarks.len() < len_before
    }

    /// Get all tracked bookmark names.
    pub fn tracked_names(&self) -> Vec<&str> {
        self.bookmarks.iter().map(|b| b.name.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tracked_bookmark_new() {
        let bookmark = TrackedBookmark::new("feat-auth".to_string(), "abc123".to_string());
        assert_eq!(bookmark.name, "feat-auth");
        assert_eq!(bookmark.change_id, "abc123");
        assert!(bookmark.remote.is_none());
    }

    #[test]
    fn test_tracked_bookmark_with_remote() {
        let bookmark = TrackedBookmark::with_remote(
            "feat-auth".to_string(),
            "abc123".to_string(),
            "upstream".to_string(),
        );
        assert_eq!(bookmark.remote, Some("upstream".to_string()));
    }

    #[test]
    fn test_tracking_state_track_untrack() {
        let mut state = TrackingState::new();
        assert!(!state.is_tracked("feat-auth"));

        state.track(TrackedBookmark::new(
            "feat-auth".to_string(),
            "abc123".to_string(),
        ));
        assert!(state.is_tracked("feat-auth"));
        assert_eq!(state.tracked_names(), vec!["feat-auth"]);

        // Duplicate track is no-op
        state.track(TrackedBookmark::new(
            "feat-auth".to_string(),
            "def456".to_string(),
        ));
        assert_eq!(state.bookmarks.len(), 1);

        assert!(state.untrack("feat-auth"));
        assert!(!state.is_tracked("feat-auth"));
        assert!(!state.untrack("feat-auth")); // Already removed
    }

    #[test]
    fn test_tracking_state_serialization() {
        let mut state = TrackingState::new();
        state.track(TrackedBookmark::new(
            "feat-auth".to_string(),
            "abc123".to_string(),
        ));

        let toml_str = toml::to_string_pretty(&state).unwrap();
        assert!(toml_str.contains("feat-auth"));
        assert!(toml_str.contains("abc123"));

        let deserialized: TrackingState = toml::from_str(&toml_str).unwrap();
        assert_eq!(deserialized.bookmarks.len(), 1);
        assert_eq!(deserialized.bookmarks[0].name, "feat-auth");
    }
}
