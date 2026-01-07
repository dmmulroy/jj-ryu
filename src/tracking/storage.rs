//! Persistence for tracking state in `.jj/repo/ryu/`.

use super::{TRACKING_VERSION, TrackingState};
use crate::error::{Error, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Directory name for ryu metadata within `.jj/repo/`.
const RYU_DIR: &str = "ryu";

/// Filename for tracking state.
const TRACKING_FILE: &str = "tracked.toml";

/// Get path to the ryu metadata directory.
fn ryu_dir(workspace_root: &Path) -> PathBuf {
    workspace_root.join(".jj").join("repo").join(RYU_DIR)
}

/// Get path to the tracking state file.
pub fn tracking_path(workspace_root: &Path) -> PathBuf {
    ryu_dir(workspace_root).join(TRACKING_FILE)
}

/// Load tracking state from disk.
///
/// Returns an empty `TrackingState` if the file doesn't exist.
pub fn load_tracking(workspace_root: &Path) -> Result<TrackingState> {
    let path = tracking_path(workspace_root);

    if !path.exists() {
        return Ok(TrackingState::new());
    }

    let content = fs::read_to_string(&path)
        .map_err(|e| Error::Tracking(format!("failed to read {}: {e}", path.display())))?;

    let state: TrackingState = toml::from_str(&content)
        .map_err(|e| Error::Tracking(format!("failed to parse {}: {e}", path.display())))?;

    Ok(state)
}

/// Save tracking state to disk.
///
/// Creates the `.jj/repo/ryu/` directory if it doesn't exist.
pub fn save_tracking(workspace_root: &Path, state: &TrackingState) -> Result<()> {
    let dir = ryu_dir(workspace_root);
    let path = dir.join(TRACKING_FILE);

    // Ensure directory exists
    if !dir.exists() {
        fs::create_dir_all(&dir)
            .map_err(|e| Error::Tracking(format!("failed to create {}: {e}", dir.display())))?;
    }

    // Serialize with version
    let mut state_to_save = state.clone();
    state_to_save.version = TRACKING_VERSION;

    let content = toml::to_string_pretty(&state_to_save)
        .map_err(|e| Error::Tracking(format!("failed to serialize tracking state: {e}")))?;

    // Add header comment
    let content_with_header = format!(
        "# ryu tracking metadata\n# Auto-generated - manual edits may be overwritten\n\n{content}"
    );

    fs::write(&path, content_with_header)
        .map_err(|e| Error::Tracking(format!("failed to write {}: {e}", path.display())))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracking::TrackedBookmark;
    use tempfile::TempDir;

    fn setup_fake_jj_workspace() -> TempDir {
        let temp = TempDir::new().unwrap();
        // Create .jj/repo directory structure
        fs::create_dir_all(temp.path().join(".jj").join("repo")).unwrap();
        temp
    }

    #[test]
    fn test_tracking_path() {
        let temp = setup_fake_jj_workspace();
        let path = tracking_path(temp.path());
        assert!(path.ends_with(".jj/repo/ryu/tracked.toml"));
    }

    #[test]
    fn test_load_missing_file_returns_empty() {
        let temp = setup_fake_jj_workspace();
        let state = load_tracking(temp.path()).unwrap();
        assert!(state.bookmarks.is_empty());
        assert_eq!(state.version, TRACKING_VERSION);
    }

    #[test]
    fn test_save_creates_directory() {
        let temp = setup_fake_jj_workspace();
        let ryu_dir = temp.path().join(".jj").join("repo").join("ryu");
        assert!(!ryu_dir.exists());

        let state = TrackingState::new();
        save_tracking(temp.path(), &state).unwrap();

        assert!(ryu_dir.exists());
        assert!(tracking_path(temp.path()).exists());
    }

    #[test]
    fn test_roundtrip_serialization() {
        let temp = setup_fake_jj_workspace();

        let mut state = TrackingState::new();
        state.track(TrackedBookmark::new(
            "feat-auth".to_string(),
            "abc123".to_string(),
        ));
        state.track(TrackedBookmark::with_remote(
            "feat-db".to_string(),
            "def456".to_string(),
            "upstream".to_string(),
        ));

        save_tracking(temp.path(), &state).unwrap();

        let loaded = load_tracking(temp.path()).unwrap();
        assert_eq!(loaded.bookmarks.len(), 2);
        assert_eq!(loaded.bookmarks[0].name, "feat-auth");
        assert_eq!(loaded.bookmarks[0].change_id, "abc123");
        assert!(loaded.bookmarks[0].remote.is_none());
        assert_eq!(loaded.bookmarks[1].name, "feat-db");
        assert_eq!(loaded.bookmarks[1].remote, Some("upstream".to_string()));
    }

    #[test]
    fn test_file_contains_header_comment() {
        let temp = setup_fake_jj_workspace();
        let state = TrackingState::new();
        save_tracking(temp.path(), &state).unwrap();

        let content = fs::read_to_string(tracking_path(temp.path())).unwrap();
        assert!(content.starts_with("# ryu tracking metadata"));
        assert!(content.contains("Auto-generated"));
    }
}
