//! Platform service factory
//!
//! Creates platform services based on configuration.

use crate::auth::{get_azure_devops_auth, get_github_auth, get_gitlab_auth};
use crate::error::Result;
use crate::platform::{AzureDevOpsService, GitHubService, GitLabService, PlatformService};
use crate::types::{Platform, PlatformConfig};

/// Create a platform service from configuration
///
/// Handles authentication and client construction for both GitHub and GitLab.
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
        Platform::AzureDevOps => {
            let auth = get_azure_devops_auth(config.host.as_deref()).await?;
            // Parse owner as org/project
            let parts: Vec<&str> = config.owner.split('/').collect();
            if parts.len() != 2 {
                return Err(crate::error::Error::Config(format!(
                    "Azure DevOps owner must be in format 'org/project', got: {}",
                    config.owner
                )));
            }
            Ok(Box::new(AzureDevOpsService::new(
                auth.token.clone(),
                parts[0].to_string(),
                parts[1].to_string(),
                config.repo.clone(),
                Some(auth.host),
            )?))
        }
    }
}
