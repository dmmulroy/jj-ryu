//! Error types for jj-ryu
//!
//! Uses thiserror for structured errors that can be mapped to HTTP status codes
//! in future web server implementations.

use thiserror::Error;

/// Main error type for jj-ryu operations
#[derive(Error, Debug)]
pub enum Error {
    /// Failed to load or interact with jj workspace
    #[error("workspace error: {0}")]
    Workspace(String),

    /// Failed to parse jj output or data
    #[error("parse error: {0}")]
    Parse(String),

    /// Bookmark not found in repository
    #[error("bookmark '{0}' not found")]
    BookmarkNotFound(String),

    /// No stack found (working copy at trunk or no bookmarks)
    #[error("{0}")]
    NoStack(String),

    /// No supported remotes (GitHub/GitLab) found
    #[error("no supported remotes found (GitHub/GitLab)")]
    NoSupportedRemotes,

    /// Specified remote not found
    #[error("remote '{0}' not found")]
    RemoteNotFound(String),

    /// Authentication failed
    #[error("authentication failed: {0}")]
    Auth(String),

    /// GitHub API error
    #[error("GitHub API error: {0}")]
    GitHubApi(String),

    /// GitLab API error
    #[error("GitLab API error: {0}")]
    GitLabApi(String),

    /// Merge commit detected (cannot stack)
    #[error("merge commit detected in bookmark '{0}' history - rebasing required")]
    MergeCommitDetected(String),

    /// Revset evaluation failed
    #[error("revset error: {0}")]
    Revset(String),

    /// Git operation failed
    #[error("git operation failed: {0}")]
    Git(String),

    /// Invalid configuration
    #[error("invalid configuration: {0}")]
    Config(String),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// HTTP request error
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// JSON serialization/deserialization error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// URL parsing error
    #[error("URL parse error: {0}")]
    UrlParse(#[from] url::ParseError),

    /// Octocrab (GitHub) error
    #[error("GitHub client error: {0}")]
    Octocrab(#[from] octocrab::Error),

    /// Platform API error (generic)
    #[error("platform error: {0}")]
    Platform(String),

    /// Internal error (unexpected state)
    #[error("internal error: {0}")]
    Internal(String),

    /// Scheduler detected a cycle in execution dependencies (indicates a bug)
    #[error("scheduler cycle detected: {message}")]
    SchedulerCycle {
        /// Human-readable description
        message: String,
        /// Node names involved in the cycle (for debugging)
        cycle_nodes: Vec<String>,
    },

    /// Invalid command-line argument
    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    /// Tracking state error
    #[error("tracking error: {0}")]
    Tracking(String),
}

/// Result type alias for jj-ryu operations
pub type Result<T> = std::result::Result<T, Error>;
