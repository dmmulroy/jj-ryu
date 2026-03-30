//! Gitea authentication

use crate::auth::AuthSource;
use crate::error::{Error, Result};
use reqwest::Client;
use serde::Deserialize;
use std::env;
use tokio::process::Command;
use tracing::debug;

/// Gitea authentication configuration
#[derive(Debug, Clone)]
pub struct GiteaAuthConfig {
    /// Authentication token
    pub token: String,
    /// Where the token was obtained from
    pub source: AuthSource,
    /// Gitea host
    pub host: String,
}

/// Resolve the target Gitea host from explicit args, then env.
pub fn resolve_gitea_host(host: Option<&str>, env_host: Option<&str>) -> String {
    host.map(str::to_string)
        .or_else(|| env_host.map(str::to_string))
        .unwrap_or_default()
}

/// Choose the highest-priority Gitea token from environment-sourced values.
pub fn pick_gitea_env_token(gitea_token: Option<&str>, gt_token: Option<&str>) -> Option<String> {
    gitea_token
        .map(str::to_string)
        .or_else(|| gt_token.map(str::to_string))
}

/// Get Gitea authentication
///
/// Priority:
/// 1. tea CLI (`tea auth token --host <host>`)
/// 2. `GITEA_TOKEN` environment variable
/// 3. `GT_TOKEN` environment variable
pub async fn get_gitea_auth(host: Option<&str>) -> Result<GiteaAuthConfig> {
    let configured_host = resolve_gitea_host(host, env::var("GITEA_HOST").ok().as_deref());
    let host = if configured_host.is_empty() {
        get_tea_default_host().await.ok_or_else(|| {
            Error::Auth(
                "No Gitea host configured. Pass a host, set GITEA_HOST, or configure a default `tea` login".to_string(),
            )
        })?
    } else {
        configured_host
    };

    debug!(host = %host, "attempting to get Gitea token");
    if let Some(token) = get_tea_cli_token(&host).await {
        return Ok(GiteaAuthConfig {
            token,
            source: AuthSource::Cli,
            host,
        });
    }

    let access_like_token = pick_gitea_env_token(
        env::var("GITEA_ACCESS_TOKEN").ok().as_deref(),
        env::var("GITEA_KEY").ok().as_deref(),
    );

    if let Some(token) = pick_gitea_env_token(
        env::var("GITEA_TOKEN").ok().as_deref(),
        access_like_token.as_deref(),
    )
    .or_else(|| env::var("GT_TOKEN").ok())
    {
        return Ok(GiteaAuthConfig {
            token,
            source: AuthSource::EnvVar,
            host,
        });
    }

    Err(Error::Auth(
        "No Gitea authentication found. Run `tea login add` or set GITEA_TOKEN".to_string(),
    ))
}

#[derive(Deserialize)]
struct TeaLogin {
    url: String,
    default: String,
}

async fn get_tea_default_host() -> Option<String> {
    Command::new("tea").arg("--version").output().await.ok()?;

    let output = Command::new("tea")
        .args(["login", "list", "-o", "json"])
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let logins: Vec<TeaLogin> = serde_json::from_slice(&output.stdout).ok()?;
    let default_login = logins.into_iter().find(|login| login.default == "true")?;
    let url = url::Url::parse(&default_login.url).ok()?;
    url.host_str().map(ToString::to_string)
}

async fn get_tea_cli_token(host: &str) -> Option<String> {
    Command::new("tea").arg("--version").output().await.ok()?;

    let output = Command::new("tea")
        .args(["auth", "token", "--host", host])
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if token.is_empty() { None } else { Some(token) }
}

#[derive(Deserialize)]
struct GiteaUser {
    login: String,
}

/// Test Gitea authentication
pub async fn test_gitea_auth(config: &GiteaAuthConfig) -> Result<String> {
    let url = format!("https://{}/api/v1/user", config.host);
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| Error::GiteaApi(format!("failed to create HTTP client: {e}")))?;

    let user: GiteaUser = client
        .get(&url)
        .header("Authorization", format!("token {}", config.token))
        .send()
        .await?
        .error_for_status()
        .map_err(|e| Error::Auth(format!("Invalid token: {e}")))?
        .json()
        .await?;

    Ok(user.login)
}
