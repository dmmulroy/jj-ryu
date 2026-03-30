//! Authentication for GitHub, GitLab, and Gitea
//!
//! Supports CLI-based auth (gh, glab) and environment variables.

mod gitea;
mod github;
mod gitlab;

pub use gitea::{
    GiteaAuthConfig, get_gitea_auth, pick_gitea_env_token, resolve_gitea_host, test_gitea_auth,
};
pub use github::{GitHubAuthConfig, get_github_auth, test_github_auth};
pub use gitlab::{GitLabAuthConfig, get_gitlab_auth, test_gitlab_auth};

/// Source of authentication token
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthSource {
    /// Token from CLI tool (gh or glab)
    Cli,
    /// Token from environment variable
    EnvVar,
}
