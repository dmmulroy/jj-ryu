//! Temporary jj repository for testing

use jj_ryu::repo::JjWorkspace;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

/// Temporary jj repository for testing
///
/// Creates a jj workspace with git backend in a temporary directory.
/// Uses the jj CLI for operations to ensure compatibility.
/// Automatically cleaned up when dropped.
pub struct TempJjRepo {
    dir: TempDir,
}

impl TempJjRepo {
    /// Create a new jj repo with internal git backend
    pub fn new() -> Self {
        let dir = TempDir::new().expect("failed to create temp directory for test repo");

        // Initialize jj workspace with git backend
        let output = Command::new("jj")
            .args(["git", "init"])
            .current_dir(dir.path())
            .output()
            .expect("jj binary not found - is jj installed and in PATH?");

        assert!(
            output.status.success(),
            "jj git init failed at {}: {}",
            dir.path().display(),
            String::from_utf8_lossy(&output.stderr)
        );

        // Create initial commit to have something to build on
        let output = Command::new("jj")
            .args(["commit", "-m", "Initial commit"])
            .current_dir(dir.path())
            .output()
            .expect("failed to run jj commit");

        assert!(
            output.status.success(),
            "jj commit failed at {}: {}",
            dir.path().display(),
            String::from_utf8_lossy(&output.stderr)
        );

        Self { dir }
    }

    /// Get the workspace root path
    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Open as `JjWorkspace` for use with jj-ryu
    pub fn workspace(&self) -> JjWorkspace {
        JjWorkspace::open(self.dir.path()).unwrap_or_else(|e| {
            panic!(
                "failed to open workspace at {}: {e}",
                self.dir.path().display()
            )
        })
    }

    /// Create a new commit with the given message
    pub fn commit(&self, message: &str) {
        let output = Command::new("jj")
            .args(["commit", "-m", message])
            .current_dir(self.dir.path())
            .output()
            .expect("failed to run jj commit");

        assert!(
            output.status.success(),
            "jj commit -m {:?} failed at {}: {}",
            message,
            self.dir.path().display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// Create a bookmark at the current working copy commit
    pub fn create_bookmark(&self, name: &str) {
        let output = Command::new("jj")
            .args(["bookmark", "create", name])
            .current_dir(self.dir.path())
            .output()
            .expect("failed to run jj bookmark create");

        assert!(
            output.status.success(),
            "jj bookmark create {:?} failed at {}: {}",
            name,
            self.dir.path().display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// Build a linear stack of commits with bookmarks
    ///
    /// Each tuple is (`bookmark_name`, `commit_message`).
    /// Creates commits in order, each on top of the previous.
    pub fn build_stack(&self, bookmarks: &[(&str, &str)]) {
        for (bookmark, message) in bookmarks {
            self.commit(message);
            self.create_bookmark(bookmark);
        }
    }

    /// Get all bookmark names in this repo
    pub fn bookmark_names(&self) -> Vec<String> {
        let ws = self.workspace();
        ws.local_bookmarks()
            .expect("get bookmarks")
            .into_iter()
            .map(|b| b.name)
            .collect()
    }

    /// Run a jj command with arguments, returning stdout on success
    #[allow(dead_code)]
    fn run_jj(&self, args: &[&str]) -> String {
        let output = Command::new("jj")
            .args(args)
            .current_dir(self.dir.path())
            .output()
            .expect("failed to run jj command");

        assert!(
            output.status.success(),
            "jj {} failed at {}: {}",
            args.join(" "),
            self.dir.path().display(),
            String::from_utf8_lossy(&output.stderr)
        );

        String::from_utf8_lossy(&output.stdout).to_string()
    }

    /// Rebase a revision before another revision
    ///
    /// Example: `rebase_before("feat-b", "feat-a")` moves feat-b to be
    /// the parent of feat-a (i.e., swaps their order if feat-a was parent of feat-b)
    #[allow(dead_code)]
    pub fn rebase_before(&self, rev: &str, before: &str) {
        self.run_jj(&["rebase", "-r", rev, "--before", before]);
    }

    /// Move a bookmark to a different revision
    #[allow(dead_code)]
    pub fn move_bookmark(&self, name: &str, to_rev: &str) {
        self.run_jj(&["bookmark", "move", name, "--to", to_rev]);
    }

    /// Edit (checkout) a revision, making it the working copy parent
    #[allow(dead_code)]
    pub fn edit(&self, rev: &str) {
        self.run_jj(&["edit", rev]);
    }

    /// Get the change ID for a bookmark
    #[allow(dead_code)]
    pub fn change_id(&self, bookmark: &str) -> String {
        let output = self.run_jj(&["log", "-r", bookmark, "--no-graph", "-T", "change_id"]);
        output.trim().to_string()
    }

    /// Create an empty commit (useful for testing without file changes)
    #[allow(dead_code)]
    pub fn empty_commit(&self, message: &str) {
        let output = Command::new("jj")
            .args(["commit", "--allow-empty", "-m", message])
            .current_dir(self.dir.path())
            .output()
            .expect("failed to run jj commit");

        // --allow-empty may not exist in all jj versions, fall back to regular commit
        if !output.status.success() {
            self.commit(message);
        }
    }
}

impl Default for TempJjRepo {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_temp_repo() {
        let repo = TempJjRepo::new();
        assert!(repo.path().exists());
        assert!(repo.path().join(".jj").exists());
    }

    #[test]
    fn test_create_bookmark() {
        let repo = TempJjRepo::new();
        repo.commit("test commit");
        repo.create_bookmark("test-bookmark");

        let names = repo.bookmark_names();
        assert!(names.contains(&"test-bookmark".to_string()));
    }

    #[test]
    fn test_build_stack() {
        let repo = TempJjRepo::new();
        repo.build_stack(&[("feat-a", "Add A"), ("feat-b", "Add B")]);

        let names = repo.bookmark_names();
        assert!(names.contains(&"feat-a".to_string()));
        assert!(names.contains(&"feat-b".to_string()));
    }

    #[test]
    fn test_open_as_workspace() {
        let repo = TempJjRepo::new();
        repo.build_stack(&[("feat-a", "Add A")]);

        let ws = repo.workspace();
        let bookmarks = ws.local_bookmarks().expect("get bookmarks");
        assert!(!bookmarks.is_empty());
    }
}
