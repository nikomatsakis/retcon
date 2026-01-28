//! Git repository operations.

use std::path::{Path, PathBuf};
use std::process::Command;

/// A git repository handle that provides common operations.
pub struct Git {
    root: PathBuf,
}

impl Git {
    /// Find the git repository root starting from the given path.
    pub fn discover(start: &Path) -> Result<Self, Error> {
        // Start from the given directory, or current dir if it's just a filename
        let start_dir = start
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));

        let output = Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .current_dir(start_dir)
            .output()
            .map_err(|e| Error::Exec(format!("git rev-parse: {e}")))?;

        if !output.status.success() {
            return Err(Error::NotARepo(start_dir.display().to_string()));
        }

        let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(Self {
            root: PathBuf::from(root),
        })
    }

    /// Get the repository root path.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Check if a branch or ref exists.
    pub fn ref_exists(&self, refname: &str) -> bool {
        Command::new("git")
            .args(["rev-parse", "--verify", refname])
            .current_dir(&self.root)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Get the merge-base between two refs.
    pub fn merge_base(&self, ref1: &str, ref2: &str) -> Result<String, Error> {
        let output = self.run_output(&["merge-base", ref1, ref2])?;
        Ok(output.trim().to_string())
    }

    /// Checkout a branch.
    pub fn checkout(&self, branch: &str) -> Result<(), Error> {
        self.run(&["checkout", branch])
    }

    /// Create and checkout a new branch from a starting point.
    pub fn checkout_new_branch(&self, branch: &str, start: &str) -> Result<(), Error> {
        self.run(&["checkout", "-b", branch, start])
    }

    /// Get the diff between two refs.
    pub fn diff(&self, from: &str, to: &str) -> Result<String, Error> {
        let range = format!("{from}..{to}");
        self.run_output(&["diff", &range])
    }

    /// Checkout files from a ref.
    pub fn checkout_files(&self, refname: &str, pathspec: &str) -> Result<(), Error> {
        self.run(&["checkout", refname, "--", pathspec])
    }

    /// Stage all changes.
    pub fn add_all(&self) -> Result<(), Error> {
        self.run(&["add", "-A"])
    }

    /// Create a commit with the given message, returning the short hash.
    pub fn commit(&self, message: &str) -> Result<String, Error> {
        self.add_all()?;
        self.run(&["commit", "-m", message])?;
        self.head_short()
    }

    /// Get the short hash of HEAD.
    pub fn head_short(&self) -> Result<String, Error> {
        let hash = self.run_output(&["rev-parse", "HEAD"])?;
        let hash = hash.trim();
        Ok(hash[..8.min(hash.len())].to_string())
    }

    // -------------------------------------------------------------------------
    // Internal helpers
    // -------------------------------------------------------------------------

    /// Run a git command that produces no output we care about.
    fn run(&self, args: &[&str]) -> Result<(), Error> {
        let status = Command::new("git")
            .args(args)
            .current_dir(&self.root)
            .status()
            .map_err(|e| Error::Exec(format!("git {}: {e}", args.first().unwrap_or(&""))))?;

        if status.success() {
            Ok(())
        } else {
            Err(Error::Failed(format!("git {}", args.join(" "))))
        }
    }

    /// Run a git command and capture its stdout.
    fn run_output(&self, args: &[&str]) -> Result<String, Error> {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.root)
            .output()
            .map_err(|e| Error::Exec(format!("git {}: {e}", args.first().unwrap_or(&""))))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(Error::Failed(format!("git {}", args.join(" "))))
        }
    }
}

/// Errors from git operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to execute: {0}")]
    Exec(String),

    #[error("not a git repository (searched from '{0}')")]
    NotARepo(String),

    #[error("{0}")]
    Failed(String),
}
