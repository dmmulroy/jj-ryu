//! Platform service factory
//!
//! Creates platform services based on configuration.

use crate::auth::{get_gitea_auth, get_github_auth, get_gitlab_auth};
use crate::error::Result;
use crate::platform::{GitHubService, GitLabService, GiteaService, PlatformService};
use crate::types::{Platform, PlatformConfig};

/// Create a platform service from configuration
///
/// Handles authentication and client construction for supported platforms.
pub async fn create_platform_service(config: &PlatformConfig) -> Result<Box<dyn PlatformService>> {
    match config.platform {
        Platform::GitHub => {
            let auth = get_github_auth().await?;
            Ok(Box::new(GitHubService::new(
                &auth.token,
                config.owner.clone(),
                config.repo.clone(),
                config.host.clone(),
            )?))
        }
        Platform::GitLab => {
            let auth = get_gitlab_auth(config.host.as_deref()).await?;
            Ok(Box::new(GitLabService::new(
                auth.token.clone(),
                config.owner.clone(),
                config.repo.clone(),
                Some(auth.host),
            )?))
        }
        Platform::Gitea => {
            let auth = get_gitea_auth(config.host.as_deref()).await?;
            Ok(Box::new(GiteaService::new(
                auth.token.clone(),
                config.owner.clone(),
                config.repo.clone(),
                Some(auth.host),
            )?))
        }
    }
}
