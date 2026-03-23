//! Git-based file snapshot and revert system.
//!
//! Uses a separate git repo with the same worktree as the user's project
//! to track file changes during agent execution. This allows undoing
//! changes without affecting the user's git history.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use tracing::info;

/// Manages snapshots using a dedicated git directory.
pub struct Snapshots {
    /// Path to the snapshot git directory (e.g., ~/.local/share/hob/snapshots/<project_hash>)
    git_dir: PathBuf,
    /// Path to the work tree (the user's project directory)
    work_tree: PathBuf,
}

impl Snapshots {
    /// Create a new snapshot manager for the given work tree.
    pub fn new(work_tree: &Path) -> Result<Self> {
        let project_hash = hash_path(work_tree);
        let data_dir = crate::store::Store::default_path()
            .parent()
            .unwrap_or(Path::new("."))
            .join("snapshots")
            .join(&project_hash);

        std::fs::create_dir_all(&data_dir)?;

        let snapshots = Self {
            git_dir: data_dir,
            work_tree: work_tree.to_path_buf(),
        };

        // Initialize the snapshot repo if needed
        if !snapshots.git_dir.join("HEAD").exists() {
            let output = Command::new("git")
                .args(["init", "--bare"])
                .arg(&snapshots.git_dir)
                .output()
                .context("failed to init snapshot repo")?;
            if !output.status.success() {
                anyhow::bail!(
                    "git init --bare failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }

        Ok(snapshots)
    }

    /// Capture the current state of all files. Returns a tree hash.
    pub fn track(&self) -> Result<String> {
        self.git(&["add", "-A"])?;
        let output = self.git(&["write-tree"])?;
        Ok(output.trim().to_string())
    }

    /// Get list of files changed since a snapshot.
    pub fn changed_files(&self, since_hash: &str) -> Result<Vec<String>> {
        self.git(&["add", "-A"])?;
        let output = self.git(&[
            "diff", "--no-ext-diff", "--name-only", since_hash,
        ])?;
        Ok(output
            .lines()
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect())
    }

    /// Revert files to a previous snapshot state.
    pub fn revert(&self, hash: &str, files: &[String]) -> Result<()> {
        for file in files {
            // Try to checkout the file from the snapshot
            let result = self.git(&["checkout", hash, "--", file]);
            if result.is_err() {
                // File might be new (doesn't exist in snapshot) — try to check
                let ls = self.git(&["ls-tree", hash, "--", file]);
                if ls.map(|s| s.trim().is_empty()).unwrap_or(true) {
                    // File was created after snapshot — delete it
                    let path = self.work_tree.join(file);
                    if path.exists() {
                        std::fs::remove_file(&path).ok();
                        info!("snapshot: deleted new file {file}");
                    }
                }
            }
        }
        Ok(())
    }

    /// Restore all files to a snapshot state.
    pub fn restore(&self, hash: &str) -> Result<()> {
        self.git(&["read-tree", hash])?;
        self.git(&["checkout-index", "-a", "-f"])?;
        Ok(())
    }

    /// Run a git command with the snapshot git dir and work tree.
    fn git(&self, args: &[&str]) -> Result<String> {
        let output = Command::new("git")
            .arg(format!("--git-dir={}", self.git_dir.display()))
            .arg(format!("--work-tree={}", self.work_tree.display()))
            .arg("-c")
            .arg("core.autocrlf=false")
            .args(args)
            .output()
            .context("failed to run git")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("git {:?} failed: {}", args, stderr.trim());
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

/// Hash a path to a stable project identifier.
fn hash_path(path: &Path) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    path.display().to_string().hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

/// Auto-checkpoint: commit changes to the user's git repo after tool execution.
/// Only works if we're inside a git repo. Non-destructive — creates a new commit.
pub fn auto_checkpoint(message: &str) -> Result<Option<String>> {
    // Check if we're in a git repo
    let status = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .output();
    match status {
        Ok(out) if out.status.success() => {}
        _ => return Ok(None), // not in a git repo, skip
    }

    // Check for changes
    let diff = Command::new("git")
        .args(["diff", "--stat", "HEAD"])
        .output()
        .context("git diff failed")?;
    let diff_output = String::from_utf8_lossy(&diff.stdout);
    if diff_output.trim().is_empty() {
        // Also check untracked
        let untracked = Command::new("git")
            .args(["ls-files", "--others", "--exclude-standard"])
            .output()
            .context("git ls-files failed")?;
        if String::from_utf8_lossy(&untracked.stdout).trim().is_empty() {
            return Ok(None); // no changes
        }
    }

    // Stage all changes
    Command::new("git")
        .args(["add", "-A"])
        .output()
        .context("git add failed")?;

    // Commit
    let commit_msg = format!("hob: {message}");
    let output = Command::new("git")
        .args(["commit", "-m", &commit_msg, "--no-verify"])
        .output()
        .context("git commit failed")?;

    if output.status.success() {
        // Get the commit hash
        let hash = Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
        info!("auto-checkpoint: {}", hash.as_deref().unwrap_or("unknown"));
        Ok(hash)
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (TempDir, Snapshots) {
        let dir = TempDir::new().unwrap();
        // Initialize a git repo so our snapshot system works
        Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        let snaps = Snapshots::new(dir.path()).unwrap();
        (dir, snaps)
    }

    #[test]
    fn test_track_returns_hash() {
        let (dir, snaps) = setup();
        std::fs::write(dir.path().join("test.txt"), "hello").unwrap();
        let hash = snaps.track().unwrap();
        assert!(!hash.is_empty());
        assert!(hash.len() >= 7); // git hash
    }

    #[test]
    fn test_changed_files_detects_modifications() {
        let (dir, snaps) = setup();
        std::fs::write(dir.path().join("a.txt"), "v1").unwrap();
        let hash = snaps.track().unwrap();

        std::fs::write(dir.path().join("a.txt"), "v2").unwrap();
        std::fs::write(dir.path().join("b.txt"), "new").unwrap();

        let changed = snaps.changed_files(&hash).unwrap();
        assert!(changed.contains(&"a.txt".to_string()));
        assert!(changed.contains(&"b.txt".to_string()));
    }

    #[test]
    fn test_revert_restores_file() {
        let (dir, snaps) = setup();
        std::fs::write(dir.path().join("a.txt"), "original").unwrap();
        let hash = snaps.track().unwrap();

        std::fs::write(dir.path().join("a.txt"), "modified").unwrap();
        snaps.revert(&hash, &["a.txt".to_string()]).unwrap();

        let content = std::fs::read_to_string(dir.path().join("a.txt")).unwrap();
        assert_eq!(content, "original");
    }

    #[test]
    fn test_revert_deletes_new_files() {
        let (dir, snaps) = setup();
        std::fs::write(dir.path().join("a.txt"), "exists").unwrap();
        let hash = snaps.track().unwrap();

        std::fs::write(dir.path().join("new.txt"), "shouldn't exist").unwrap();
        snaps.revert(&hash, &["new.txt".to_string()]).unwrap();

        assert!(!dir.path().join("new.txt").exists());
    }

    #[test]
    fn test_snapshot_dir_created() {
        let dir = TempDir::new().unwrap();
        Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        let snaps = Snapshots::new(dir.path()).unwrap();
        assert!(snaps.git_dir.join("HEAD").exists());
    }

    #[test]
    fn test_auto_checkpoint_no_changes() {
        let dir = TempDir::new().unwrap();
        Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        // Configure git user for the test repo
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        // Create initial commit
        std::fs::write(dir.path().join("init.txt"), "init").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        // No changes — checkpoint should return None
        let saved = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        let result = auto_checkpoint("test");
        std::env::set_current_dir(saved).unwrap();
        assert!(result.unwrap().is_none());
    }
}
