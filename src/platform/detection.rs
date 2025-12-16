//! Platform detection from remote URLs

use crate::error::{Error, Result};
use crate::types::{Platform, PlatformConfig};
use regex::Regex;
use std::env;
use std::sync::LazyLock;

/// Regex for SSH URLs: git@host:owner/repo.git
static RE_SSH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"git@[^:]+:(.+?)(?:\.git)?$").unwrap());

/// Regex for HTTPS URLs: https://host/owner/repo.git
static RE_HTTPS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"https?://[^/]+/(.+?)(?:\.git)?$").unwrap());

/// Detect platform (GitHub or GitLab) from a remote URL
pub fn detect_platform(url: &str) -> Option<Platform> {
    let gh_host = env::var("GH_HOST").ok();
    let gitlab_host = env::var("GITLAB_HOST").ok();

    let hostname = extract_hostname(url)?;

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
    };

    Ok(PlatformConfig {
        platform,
        owner,
        repo,
        host,
    })
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
}
