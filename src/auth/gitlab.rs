//! GitLab authentication

use crate::auth::AuthSource;
use crate::error::{Error, Result};
use crate::types::normalize_host;
use reqwest::Client;
use serde::Deserialize;
use std::env;
use tokio::process::Command;
use tracing::debug;

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
/// 1. glab CLI (`glab auth status --show-token`)
/// 2. `GITLAB_TOKEN` environment variable
/// 3. `GL_TOKEN` environment variable
pub async fn get_gitlab_auth(host: Option<&str>) -> Result<GitLabAuthConfig> {
    let host = host
        .map(normalize_host)
        .or_else(|| env::var("GITLAB_HOST").ok().map(|h| normalize_host(&h)))
        .unwrap_or_else(|| "gitlab.com".to_string());

    // Try glab CLI first
    debug!(host = %host, "attempting to get GitLab token via glab CLI");
    if let Some(token) = get_glab_cli_token(&host).await {
        debug!("obtained GitLab token from glab CLI");
        return Ok(GitLabAuthConfig {
            token,
            source: AuthSource::Cli,
            host,
        });
    }

    // Try environment variables
    debug!("glab CLI token not available, checking env vars");
    if let Ok(token) = env::var("GITLAB_TOKEN") {
        debug!("obtained GitLab token from GITLAB_TOKEN env var");
        return Ok(GitLabAuthConfig {
            token,
            source: AuthSource::EnvVar,
            host,
        });
    }

    if let Ok(token) = env::var("GL_TOKEN") {
        debug!("obtained GitLab token from GL_TOKEN env var");
        return Ok(GitLabAuthConfig {
            token,
            source: AuthSource::EnvVar,
            host,
        });
    }

    debug!("no GitLab authentication found");
    Err(Error::Auth(
        "No GitLab authentication found. Run `glab auth login` or set GITLAB_TOKEN".to_string(),
    ))
}

async fn get_glab_cli_token(host: &str) -> Option<String> {
    let output = Command::new("glab")
        .args(["auth", "status", "--show-token", "--hostname", host])
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    parse_glab_token_from_status(&format!("{stdout}\n{stderr}"))
}

/// Parse an unmasked token from current or older `glab auth status --show-token` output.
pub fn parse_glab_token_from_status(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let token = line
            .split_once("Token found in ")
            .and_then(|(_, source_and_token)| source_and_token.split_once(':'))
            .map(|(_, token)| token)
            .or_else(|| line.split_once("Token found:").map(|(_, token)| token))?
            .trim();

        if token.is_empty() || token.chars().all(|character| character == '*') {
            None
        } else {
            Some(token.to_string())
        }
    })
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
