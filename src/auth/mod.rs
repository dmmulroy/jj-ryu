//! Authentication for GitHub and GitLab
//!
//! Supports CLI-based auth (gh, glab) and environment variables.

mod github;
mod gitlab;

pub use github::{GitHubAuthConfig, get_github_auth, test_github_auth};
pub use gitlab::{
    GitLabAuthConfig, get_gitlab_auth, parse_glab_token_from_status, test_gitlab_auth,
};

/// Source of authentication token
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthSource {
    /// Token from CLI tool (gh or glab)
    Cli,
    /// Token from environment variable
    EnvVar,
}
