//! GitLab authentication

use crate::auth::AuthSource;
use crate::error::{Error, Result};
use reqwest::Client;
use serde::Deserialize;
use std::env;
use tokio::process::Command;

/// GitLab authentication configuration
#[derive(Debug, Clone)]
pub struct GitLabAuthConfig {
    /// Authentication token
    pub token: String,
    /// Where the token was obtained from
    pub source: AuthSource,
    /// GitLab host (e.g., "gitlab.com")
    pub host: String,
}

/// Get GitLab authentication
///
/// Priority:
/// 1. glab CLI (`glab auth token`)
/// 2. `GITLAB_TOKEN` environment variable
/// 3. `GL_TOKEN` environment variable
pub async fn get_gitlab_auth(host: Option<&str>) -> Result<GitLabAuthConfig> {
    let host = host
        .map(String::from)
        .or_else(|| env::var("GITLAB_HOST").ok())
        .unwrap_or_else(|| "gitlab.com".to_string());

    // Try glab CLI first
    if let Some(token) = get_glab_cli_token(&host).await {
        return Ok(GitLabAuthConfig {
            token,
            source: AuthSource::Cli,
            host,
        });
    }

    // Try environment variables
    if let Ok(token) = env::var("GITLAB_TOKEN") {
        return Ok(GitLabAuthConfig {
            token,
            source: AuthSource::EnvVar,
            host,
        });
    }

    if let Ok(token) = env::var("GL_TOKEN") {
        return Ok(GitLabAuthConfig {
            token,
            source: AuthSource::EnvVar,
            host,
        });
    }

    Err(Error::Auth(
        "No GitLab authentication found. Run `glab auth login` or set GITLAB_TOKEN".to_string(),
    ))
}

async fn get_glab_cli_token(host: &str) -> Option<String> {
    // Check glab is available
    Command::new("glab")
        .arg("--version")
        .output()
        .await
        .ok()?;

    // Check authenticated
    let status = Command::new("glab")
        .args(["auth", "status", "--hostname", host])
        .output()
        .await
        .ok()?;

    if !status.status.success() {
        return None;
    }

    // Get token
    let output = Command::new("glab")
        .args(["auth", "token", "--hostname", host])
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

#[derive(Deserialize)]
struct GitLabUser {
    username: String,
}

/// Test GitLab authentication
pub async fn test_gitlab_auth(config: &GitLabAuthConfig) -> Result<String> {
    let url = format!("https://{}/api/v4/user", config.host);

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| Error::GitLabApi(format!("failed to create HTTP client: {e}")))?;

    let user: GitLabUser = client
        .get(&url)
        .header("PRIVATE-TOKEN", &config.token)
        .send()
        .await?
        .error_for_status()
        .map_err(|e| Error::Auth(format!("Invalid token: {e}")))?
        .json()
        .await?;

    Ok(user.username)
}
