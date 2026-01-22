//! Platform detection from remote URLs

use crate::error::{Error, Result};
use crate::types::{Platform, PlatformConfig};
use regex::Regex;
use serde_yaml::Value;
use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Regex for SSH URLs: git@host:owner/repo.git
static RE_SSH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"git@[^:]+:(.+?)(?:\.git)?$").unwrap());

/// Regex for HTTPS URLs: `https://host/owner/repo.git`
static RE_HTTPS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"https?://[^/]+/(.+?)(?:\.git)?$").unwrap());

/// Detect platform (GitHub or GitLab) from a remote URL
pub fn detect_platform(url: &str) -> Option<Platform> {
    let gh_host = env::var("GH_HOST").ok();
    let gitlab_host = env::var("GITLAB_HOST").ok();

    let hostname = extract_hostname(url)?;
    let cli_hosts = load_cli_hosts();

    // Check GitHub
    if hostname == "github.com"
        || hostname.ends_with(".github.com")
        || gh_host.as_ref().is_some_and(|h| hostname == *h)
        || cli_hosts.github.contains(&hostname)
    {
        return Some(Platform::GitHub);
    }

    // Check GitLab
    if hostname == "gitlab.com"
        || hostname.ends_with(".gitlab.com")
        || gitlab_host.as_ref().is_some_and(|h| hostname == *h)
        || cli_hosts.gitlab.contains(&hostname)
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

    let repo = (*parts.last().unwrap()).to_string();
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

#[derive(Default)]
struct CliHosts {
    github: HashSet<String>,
    gitlab: HashSet<String>,
}

fn load_cli_hosts() -> CliHosts {
    let mut candidates = Vec::new();

    if let Ok(xdg_config_home) = env::var("XDG_CONFIG_HOME") {
        candidates.push(PathBuf::from(xdg_config_home));
    }

    if let Some(config_dir) = dirs::config_dir() {
        candidates.push(config_dir);
    }

    if let Some(home_dir) = dirs::home_dir() {
        candidates.push(home_dir.join(".config"));
    }

    load_cli_hosts_from_dirs(candidates)
}

fn load_cli_hosts_from_dirs<I>(dirs: I) -> CliHosts
where
    I: IntoIterator<Item = PathBuf>,
{
    let mut hosts = CliHosts::default();
    let mut seen = HashSet::new();

    for dir in dirs {
        if !seen.insert(dir.clone()) {
            continue;
        }

        add_github_hosts(&dir.join("gh/hosts.yml"), &mut hosts.github);
        add_gitlab_hosts(&dir.join("glab-cli/config.yml"), &mut hosts.gitlab);
    }

    hosts
}

fn add_github_hosts(path: &Path, hosts: &mut HashSet<String>) {
    let Ok(contents) = fs::read_to_string(path) else {
        return;
    };
    let Ok(value) = serde_yaml::from_str::<Value>(&contents) else {
        return;
    };

    let Value::Mapping(map) = value else {
        return;
    };

    for (key, _) in map {
        if let Value::String(host) = key {
            hosts.insert(host);
        }
    }
}

fn add_gitlab_hosts(path: &Path, hosts: &mut HashSet<String>) {
    let Ok(contents) = fs::read_to_string(path) else {
        return;
    };
    let Ok(value) = serde_yaml::from_str::<Value>(&contents) else {
        return;
    };

    let Value::Mapping(map) = value else {
        return;
    };

    if let Some(Value::String(default_host)) = map.get(Value::String("host".to_string())) {
        hosts.insert(default_host.clone());
    }

    let Some(Value::Mapping(hosts_map)) = map.get(Value::String("hosts".to_string())) else {
        return;
    };

    for (key, value) in hosts_map {
        if let Value::String(host) = key {
            hosts.insert(host.clone());
        }

        let Value::Mapping(details) = value else {
            continue;
        };

        if let Some(Value::String(api_host)) =
            details.get(Value::String("api_host".to_string()))
        {
            hosts.insert(api_host.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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
    fn test_load_cli_hosts_from_glab_config() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path();
        let glab_dir = config_dir.join("glab-cli");
        fs::create_dir_all(&glab_dir).unwrap();
        fs::write(
            glab_dir.join("config.yml"),
            "host: git.example.com\nhosts:\n  git.example.com:\n    api_host: api.git.example.com\n",
        )
        .unwrap();

        let hosts = load_cli_hosts_from_dirs([config_dir.to_path_buf()]);
        assert!(hosts.gitlab.contains("git.example.com"));
        assert!(hosts.gitlab.contains("api.git.example.com"));
    }

    #[test]
    fn test_load_cli_hosts_from_gh_config() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path();
        let gh_dir = config_dir.join("gh");
        fs::create_dir_all(&gh_dir).unwrap();
        fs::write(
            gh_dir.join("hosts.yml"),
            "github.com:\n  user: octo\n  oauth_token: token\n",
        )
        .unwrap();

        let hosts = load_cli_hosts_from_dirs([config_dir.to_path_buf()]);
        assert!(hosts.github.contains("github.com"));
    }
}
