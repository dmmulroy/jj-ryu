//! Azure DevOps authentication

use crate::auth::AuthSource;
use crate::error::{Error, Result};
use base64::Engine;
use reqwest::Client;
use serde::Deserialize;
use std::env;
use tokio::process::Command;
use tracing::debug;

/// Azure DevOps authentication configuration
#[derive(Debug, Clone)]
pub struct AzureDevOpsAuthConfig {
    /// Authentication token (Personal Access Token)
    pub token: String,
    /// Where the token was obtained from
    pub source: AuthSource,
    /// Azure DevOps host (e.g., "dev.azure.com")
    pub host: String,
    /// Optional organization for validation
    pub organization: Option<String>,
}

/// Get Azure DevOps authentication
///
/// Priority:
/// 1. `AZURE_DEVOPS_PAT` environment variable (recommended)
/// 2. `AZURE_DEVOPS_TOKEN` environment variable
/// 3. az devops CLI (`az devops configure --defaults`)
pub async fn get_azure_devops_auth(host: Option<&str>) -> Result<AzureDevOpsAuthConfig> {
    let host = host
        .map(String::from)
        .or_else(|| env::var("AZURE_DEVOPS_HOST").ok())
        .unwrap_or_else(|| "dev.azure.com".to_string());

    // Try to get organization from environment for validation
    let organization = env::var("AZURE_DEVOPS_ORGANIZATION").ok();

    // Try environment variables first (most common and reliable)
    debug!("checking AZURE_DEVOPS_PAT env var");
    if let Ok(token) = env::var("AZURE_DEVOPS_PAT") {
        debug!("obtained Azure DevOps PAT from AZURE_DEVOPS_PAT env var");
        return Ok(AzureDevOpsAuthConfig {
            token: token.trim().to_string(), // Trim whitespace
            source: AuthSource::EnvVar,
            host,
            organization,
        });
    }

    debug!("checking AZURE_DEVOPS_TOKEN env var");
    if let Ok(token) = env::var("AZURE_DEVOPS_TOKEN") {
        debug!("obtained Azure DevOps token from AZURE_DEVOPS_TOKEN env var");
        return Ok(AzureDevOpsAuthConfig {
            token: token.trim().to_string(), // Trim whitespace
            source: AuthSource::EnvVar,
            host,
            organization,
        });
    }

    // Try az devops CLI as fallback
    debug!(host = %host, "environment variables not found, attempting to get Azure DevOps token via az CLI");
    if let Some(token) = get_az_cli_token().await {
        debug!("obtained Azure DevOps token from az CLI");
        return Ok(AzureDevOpsAuthConfig {
            token,
            source: AuthSource::Cli,
            host,
            organization,
        });
    }

    debug!("no Azure DevOps authentication found");
    Err(Error::Auth(
        "No Azure DevOps authentication found. Create a PAT at https://dev.azure.com/{org}/_usersSettings/tokens and set AZURE_DEVOPS_PAT".to_string(),
    ))
}

async fn get_az_cli_token() -> Option<String> {
    // Check az is available
    Command::new("az").arg("--version").output().await.ok()?;

    // Check if Azure DevOps extension is configured
    let output = Command::new("az")
        .args(["devops", "configure", "--list"])
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    // Try to extract PAT from defaults
    // Note: This is best-effort; az CLI doesn't store PATs by default
    // Users should use environment variables for reliable authentication
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.contains("token") && line.contains('=') {
            let parts: Vec<&str> = line.split('=').collect();
            if parts.len() == 2 {
                let token = parts[1].trim().to_string();
                if !token.is_empty() {
                    return Some(token);
                }
            }
        }
    }

    None
}


/// Test Azure DevOps authentication
pub async fn test_azure_devops_auth(config: &AzureDevOpsAuthConfig) -> Result<String> {
    // If we have an organization, use the organization-scoped endpoint
    // Otherwise use the profile endpoint which works without organization
    let url = if let Some(ref org) = config.organization {
        format!("https://{}/{}/_apis/connectionData?api-version=7.1-preview", config.host, org)
    } else {
        // Use profile endpoint as fallback (works at account level)
        format!("https://app.vssps.visualstudio.com/_apis/profile/profiles/me?api-version=7.1-preview")
    };

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| Error::AzureDevOpsApi(format!("failed to create HTTP client: {e}")))?;

    // Azure DevOps uses Basic auth with empty username and PAT as password
    let auth_header = format!(":{}", config.token);
    let encoded = base64::engine::general_purpose::STANDARD.encode(auth_header);

    let response: serde_json::Value = client
        .get(&url)
        .header("Authorization", format!("Basic {encoded}"))
        .send()
        .await?
        .error_for_status()
        .map_err(|e| Error::Auth(format!("Invalid token: {e}")))?
        .json()
        .await?;

    // Extract authenticated user display name based on endpoint used
    let display_name = if config.organization.is_some() {
        response
            .get("authenticatedUser")
            .and_then(|u| u.get("providerDisplayName"))
            .and_then(|n| n.as_str())
            .unwrap_or("Unknown User")
            .to_string()
    } else {
        response
            .get("displayName")
            .and_then(|n| n.as_str())
            .unwrap_or("Unknown User")
            .to_string()
    };

    Ok(display_name)
}
