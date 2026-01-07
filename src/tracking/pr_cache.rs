//! PR association cache for tracking bookmark → PR mappings.
//!
//! The cache is stored in `.jj/repo/ryu/pr_cache.toml` and can be safely
//! deleted - it will be rebuilt on the next submit.

use crate::error::{Error, Result};
use crate::types::PullRequest;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Current version of the PR cache file format.
pub const PR_CACHE_VERSION: u32 = 1;

/// Filename for PR cache.
const PR_CACHE_FILE: &str = "pr_cache.toml";

/// A cached PR association.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CachedPr {
    /// Bookmark name this PR is associated with.
    pub bookmark: String,
    /// PR/MR number.
    pub number: u64,
    /// Web URL for the PR.
    pub url: String,
    /// Remote this PR was pushed to.
    pub remote: String,
    /// When this cache entry was last updated.
    pub updated_at: DateTime<Utc>,
}

/// PR cache state.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PrCache {
    /// File format version.
    pub version: u32,
    /// Cached PR associations.
    #[serde(default)]
    pub prs: Vec<CachedPr>,
}

impl PrCache {
    /// Create a new empty PR cache.
    pub const fn new() -> Self {
        Self {
            version: PR_CACHE_VERSION,
            prs: Vec::new(),
        }
    }

    /// Get cached PR for a bookmark.
    pub fn get(&self, bookmark: &str) -> Option<&CachedPr> {
        self.prs.iter().find(|p| p.bookmark == bookmark)
    }

    /// Update or insert a PR cache entry.
    pub fn upsert(&mut self, bookmark: &str, pr: &PullRequest, remote: &str) {
        let entry = CachedPr {
            bookmark: bookmark.to_string(),
            number: pr.number,
            url: pr.html_url.clone(),
            remote: remote.to_string(),
            updated_at: Utc::now(),
        };

        if let Some(existing) = self.prs.iter_mut().find(|p| p.bookmark == bookmark) {
            *existing = entry;
        } else {
            self.prs.push(entry);
        }
    }

    /// Remove a bookmark's PR cache entry.
    pub fn remove(&mut self, bookmark: &str) -> bool {
        let len_before = self.prs.len();
        self.prs.retain(|p| p.bookmark != bookmark);
        self.prs.len() < len_before
    }

    /// Remove entries for bookmarks not in the provided list.
    pub fn retain_bookmarks(&mut self, bookmarks: &[&str]) {
        self.prs
            .retain(|p| bookmarks.contains(&p.bookmark.as_str()));
    }
}

/// Get path to the PR cache file.
pub fn pr_cache_path(workspace_root: &Path) -> PathBuf {
    workspace_root
        .join(".jj")
        .join("repo")
        .join("ryu")
        .join(PR_CACHE_FILE)
}

/// Load PR cache from disk.
///
/// Returns an empty `PrCache` if the file doesn't exist.
pub fn load_pr_cache(workspace_root: &Path) -> Result<PrCache> {
    let path = pr_cache_path(workspace_root);

    if !path.exists() {
        return Ok(PrCache::new());
    }

    let content = fs::read_to_string(&path)
        .map_err(|e| Error::Tracking(format!("failed to read {}: {e}", path.display())))?;

    let cache: PrCache = toml::from_str(&content)
        .map_err(|e| Error::Tracking(format!("failed to parse {}: {e}", path.display())))?;

    Ok(cache)
}

/// Save PR cache to disk.
///
/// Creates the `.jj/repo/ryu/` directory if it doesn't exist.
pub fn save_pr_cache(workspace_root: &Path, cache: &PrCache) -> Result<()> {
    let path = pr_cache_path(workspace_root);
    let dir = path.parent().expect("path has parent");

    // Ensure directory exists
    if !dir.exists() {
        fs::create_dir_all(dir)
            .map_err(|e| Error::Tracking(format!("failed to create {}: {e}", dir.display())))?;
    }

    // Serialize with version
    let mut cache_to_save = cache.clone();
    cache_to_save.version = PR_CACHE_VERSION;

    let content = toml::to_string_pretty(&cache_to_save)
        .map_err(|e| Error::Tracking(format!("failed to serialize PR cache: {e}")))?;

    // Add header comment
    let content_with_header = format!(
        "# PR association cache - regenerated from platform API on submit\n\
         # Safe to delete; will be rebuilt on next submit\n\n{content}"
    );

    fs::write(&path, content_with_header)
        .map_err(|e| Error::Tracking(format!("failed to write {}: {e}", path.display())))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_fake_jj_workspace() -> TempDir {
        let temp = TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join(".jj").join("repo")).unwrap();
        temp
    }

    fn make_test_pr(number: u64) -> PullRequest {
        PullRequest {
            number,
            html_url: format!("https://github.com/owner/repo/pull/{number}"),
            base_ref: "main".to_string(),
            head_ref: "feat".to_string(),
            title: "Test PR".to_string(),
            node_id: None,
            is_draft: false,
        }
    }

    #[test]
    fn test_pr_cache_path() {
        let temp = setup_fake_jj_workspace();
        let path = pr_cache_path(temp.path());
        assert!(path.ends_with(".jj/repo/ryu/pr_cache.toml"));
    }

    #[test]
    fn test_load_missing_file_returns_empty() {
        let temp = setup_fake_jj_workspace();
        let cache = load_pr_cache(temp.path()).unwrap();
        assert!(cache.prs.is_empty());
        assert_eq!(cache.version, PR_CACHE_VERSION);
    }

    #[test]
    fn test_upsert_and_get() {
        let mut cache = PrCache::new();
        let pr = make_test_pr(123);

        cache.upsert("feat-auth", &pr, "origin");

        let cached = cache.get("feat-auth").unwrap();
        assert_eq!(cached.number, 123);
        assert_eq!(cached.remote, "origin");
        assert!(cached.url.contains("123"));

        // Update existing
        let pr2 = make_test_pr(456);
        cache.upsert("feat-auth", &pr2, "upstream");

        let cached = cache.get("feat-auth").unwrap();
        assert_eq!(cached.number, 456);
        assert_eq!(cached.remote, "upstream");
    }

    #[test]
    fn test_remove() {
        let mut cache = PrCache::new();
        cache.upsert("feat-auth", &make_test_pr(123), "origin");
        cache.upsert("feat-db", &make_test_pr(124), "origin");

        assert!(cache.remove("feat-auth"));
        assert!(cache.get("feat-auth").is_none());
        assert!(cache.get("feat-db").is_some());

        assert!(!cache.remove("feat-auth")); // Already removed
    }

    #[test]
    fn test_retain_bookmarks() {
        let mut cache = PrCache::new();
        cache.upsert("feat-auth", &make_test_pr(123), "origin");
        cache.upsert("feat-db", &make_test_pr(124), "origin");
        cache.upsert("feat-ui", &make_test_pr(125), "origin");

        cache.retain_bookmarks(&["feat-auth", "feat-ui"]);

        assert!(cache.get("feat-auth").is_some());
        assert!(cache.get("feat-db").is_none());
        assert!(cache.get("feat-ui").is_some());
    }

    #[test]
    fn test_roundtrip_serialization() {
        let temp = setup_fake_jj_workspace();

        let mut cache = PrCache::new();
        cache.upsert("feat-auth", &make_test_pr(123), "origin");
        cache.upsert("feat-db", &make_test_pr(124), "upstream");

        save_pr_cache(temp.path(), &cache).unwrap();

        let loaded = load_pr_cache(temp.path()).unwrap();
        assert_eq!(loaded.prs.len(), 2);

        let auth = loaded.get("feat-auth").unwrap();
        assert_eq!(auth.number, 123);
        assert_eq!(auth.remote, "origin");

        let db = loaded.get("feat-db").unwrap();
        assert_eq!(db.number, 124);
        assert_eq!(db.remote, "upstream");
    }

    #[test]
    fn test_file_contains_header_comment() {
        let temp = setup_fake_jj_workspace();
        let cache = PrCache::new();
        save_pr_cache(temp.path(), &cache).unwrap();

        let content = fs::read_to_string(pr_cache_path(temp.path())).unwrap();
        assert!(content.contains("PR association cache"));
        assert!(content.contains("Safe to delete"));
    }
}
