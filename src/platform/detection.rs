//! Platform detection from remote URLs

use crate::error::{Error, Result};
use crate::types::{Platform, PlatformConfig};
use regex::Regex;
use std::env;
use std::sync::LazyLock;

/// Regex for SSH URLs: git@host:owner/repo.git
static RE_SSH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"git@[^:]+:(.+?)(?:\.git)?$").unwrap());

/// Regex for HTTPS URLs: `https://host/owner/repo.git`
static RE_HTTPS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"https?://[^/]+/(.+?)(?:\.git)?$").unwrap());

/// Regex for Azure DevOps SSH URLs: git@ssh.dev.azure.com:v3/{org}/{project}/{repo}
static RE_AZURE_SSH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"git@ssh\.dev\.azure\.com:v3/([^/]+)/([^/]+)/(.+?)(?:\.git)?$").unwrap());

/// Regex for Azure DevOps HTTPS URLs: `<https://dev.azure.com/{org}/{project}/_git/{repo}>`
/// Supports optional username prefix and URL-encoded characters
static RE_AZURE_HTTPS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"https?://(?:[^@]+@)?dev\.azure\.com/([^/]+)/([^/]+)/_git/(.+?)(?:\.git)?$").unwrap());

/// Detect platform (GitHub or GitLab) from a remote URL
pub fn detect_platform(url: &str) -> Option<Platform> {
    let gh_host = env::var("GH_HOST").ok();
    let gitlab_host = env::var("GITLAB_HOST").ok();
    let azure_host = env::var("AZURE_DEVOPS_HOST").ok();

    // Check Azure DevOps patterns first (more specific)
    if RE_AZURE_SSH.is_match(url) || RE_AZURE_HTTPS.is_match(url) {
        return Some(Platform::AzureDevOps);
    }

    let hostname = extract_hostname(url)?;

    // Check Azure DevOps hostnames
    if hostname == "dev.azure.com"
        || hostname == "ssh.dev.azure.com"
        || azure_host.as_ref().is_some_and(|h| hostname == *h)
    {
        return Some(Platform::AzureDevOps);
    }

    // Check GitHub
    if hostname == "github.com"
        || hostname.ends_with(".github.com")
        || gh_host.as_ref().is_some_and(|h| hostname == *h)
    {
        return Some(Platform::GitHub);
    }

    // Check GitLab
    if hostname == "gitlab.com"
        || hostname.ends_with(".gitlab.com")
        || gitlab_host.as_ref().is_some_and(|h| hostname == *h)
    {
        return Some(Platform::GitLab);
    }

    None
}

/// Parse repository info (owner/repo) from a remote URL
pub fn parse_repo_info(url: &str) -> Result<PlatformConfig> {
    // Normalize: strip trailing slashes
    let url = url.trim_end_matches('/');

    let platform = detect_platform(url).ok_or(Error::NoSupportedRemotes)?;

    // Handle Azure DevOps specially (different structure)
    if platform == Platform::AzureDevOps {
        return parse_azure_devops_url(url);
    }

    let hostname = extract_hostname(url);

    let path = RE_SSH
        .captures(url)
        .or_else(|| RE_HTTPS.captures(url))
        .and_then(|c| c.get(1))
        .map(|m| m.as_str())
        .ok_or_else(|| Error::Parse(format!("cannot parse remote URL: {url}")))?;

    // Split path into owner and repo (GitLab supports nested groups)
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() < 2 {
        return Err(Error::Parse(format!("invalid repo path: {path}")));
    }

    let repo = parts.last().unwrap().to_string();
    let owner = parts[..parts.len() - 1].join("/");

    // Determine if self-hosted
    let host = match platform {
        Platform::GitHub => {
            if hostname.as_ref().is_some_and(|h| h != "github.com") {
                hostname
            } else {
                None
            }
        }
        Platform::GitLab => {
            if hostname.as_ref().is_some_and(|h| h != "gitlab.com") {
                hostname
            } else {
                None
            }
        }
        Platform::AzureDevOps => unreachable!("Azure DevOps handled above"),
    };

    Ok(PlatformConfig {
        platform,
        owner,
        repo,
        host,
    })
}

fn parse_azure_devops_url(url: &str) -> Result<PlatformConfig> {
    // Try SSH format first: git@ssh.dev.azure.com:v3/{org}/{project}/{repo}
    if let Some(caps) = RE_AZURE_SSH.captures(url) {
        let org = urlencoding::decode(caps.get(1).unwrap().as_str())
            .map_err(|e| Error::Parse(format!("invalid URL encoding in org: {e}")))?;
        let project = urlencoding::decode(caps.get(2).unwrap().as_str())
            .map_err(|e| Error::Parse(format!("invalid URL encoding in project: {e}")))?;
        let repo = urlencoding::decode(caps.get(3).unwrap().as_str())
            .map_err(|e| Error::Parse(format!("invalid URL encoding in repo: {e}")))?;

        return Ok(PlatformConfig {
            platform: Platform::AzureDevOps,
            owner: format!("{org}/{project}"),
            repo: repo.to_string(),
            host: None,
        });
    }

    // Try HTTPS format: https://dev.azure.com/{org}/{project}/_git/{repo}
    if let Some(caps) = RE_AZURE_HTTPS.captures(url) {
        let org = urlencoding::decode(caps.get(1).unwrap().as_str())
            .map_err(|e| Error::Parse(format!("invalid URL encoding in org: {e}")))?;
        let project = urlencoding::decode(caps.get(2).unwrap().as_str())
            .map_err(|e| Error::Parse(format!("invalid URL encoding in project: {e}")))?;
        let repo = urlencoding::decode(caps.get(3).unwrap().as_str())
            .map_err(|e| Error::Parse(format!("invalid URL encoding in repo: {e}")))?;

        return Ok(PlatformConfig {
            platform: Platform::AzureDevOps,
            owner: format!("{org}/{project}"),
            repo: repo.to_string(),
            host: None,
        });
    }

    Err(Error::Parse(format!(
        "cannot parse Azure DevOps URL: {url}"
    )))
}

fn extract_hostname(url: &str) -> Option<String> {
    // SSH format
    if url.starts_with("git@") {
        return url
            .strip_prefix("git@")
            .and_then(|s| s.split(':').next())
            .map(ToString::to_string);
    }

    // HTTPS format
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(ToString::to_string))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_github_https() {
        assert_eq!(
            detect_platform("https://github.com/owner/repo.git"),
            Some(Platform::GitHub)
        );
    }

    #[test]
    fn test_detect_github_ssh() {
        assert_eq!(
            detect_platform("git@github.com:owner/repo.git"),
            Some(Platform::GitHub)
        );
    }

    #[test]
    fn test_detect_gitlab_https() {
        assert_eq!(
            detect_platform("https://gitlab.com/owner/repo.git"),
            Some(Platform::GitLab)
        );
    }

    #[test]
    fn test_parse_github_repo() {
        let config = parse_repo_info("https://github.com/owner/repo.git").unwrap();
        assert_eq!(config.platform, Platform::GitHub);
        assert_eq!(config.owner, "owner");
        assert_eq!(config.repo, "repo");
        assert!(config.host.is_none());
    }

    #[test]
    fn test_parse_gitlab_nested_groups() {
        let config = parse_repo_info("https://gitlab.com/group/subgroup/repo.git").unwrap();
        assert_eq!(config.platform, Platform::GitLab);
        assert_eq!(config.owner, "group/subgroup");
        assert_eq!(config.repo, "repo");
    }

    #[test]
    fn test_detect_azure_devops_https() {
        assert_eq!(
            detect_platform("https://dev.azure.com/myorg/myproject/_git/myrepo"),
            Some(Platform::AzureDevOps)
        );
    }

    #[test]
    fn test_detect_azure_devops_ssh() {
        assert_eq!(
            detect_platform("git@ssh.dev.azure.com:v3/myorg/myproject/myrepo"),
            Some(Platform::AzureDevOps)
        );
    }

    #[test]
    fn test_parse_azure_devops_https() {
        let config =
            parse_repo_info("https://dev.azure.com/myorg/myproject/_git/myrepo.git").unwrap();
        assert_eq!(config.platform, Platform::AzureDevOps);
        assert_eq!(config.owner, "myorg/myproject");
        assert_eq!(config.repo, "myrepo");
        assert!(config.host.is_none());
    }

    #[test]
    fn test_parse_azure_devops_ssh() {
        let config =
            parse_repo_info("git@ssh.dev.azure.com:v3/myorg/myproject/myrepo.git").unwrap();
        assert_eq!(config.platform, Platform::AzureDevOps);
        assert_eq!(config.owner, "myorg/myproject");
        assert_eq!(config.repo, "myrepo");
        assert!(config.host.is_none());
    }

    #[test]
    fn test_parse_azure_devops_with_username() {
        // Test URL with username prefix
        let config =
            parse_repo_info("https://user@dev.azure.com/myorg/myproject/_git/myrepo")
                .unwrap();
        assert_eq!(config.platform, Platform::AzureDevOps);
        assert_eq!(config.owner, "myorg/myproject");
        assert_eq!(config.repo, "myrepo");
        assert!(config.host.is_none());
    }

    #[test]
    fn test_parse_azure_devops_with_url_encoding() {
        // Test URL with URL-encoded space in project name
        let config =
            parse_repo_info("https://dev.azure.com/myorg/My%20Project/_git/myrepo.git")
                .unwrap();
        assert_eq!(config.platform, Platform::AzureDevOps);
        assert_eq!(config.owner, "myorg/My Project");
        assert_eq!(config.repo, "myrepo");
        assert!(config.host.is_none());
    }
}
