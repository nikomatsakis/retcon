//! History specification types.
//!
//! The spec is a TOML file that serves as both the plan AND execution state.
//! As pravda works, it appends to the `history` field of each commit.

use serde::{Deserialize, Serialize};

/// The complete history specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistorySpec {
    /// Branch containing all changes (the messy history)
    pub source: String,

    /// Upstream branch this will merge into (e.g., `origin/main`)
    pub remote: String,

    /// New branch to create with reconstructed history
    pub cleaned: String,

    /// Commits to create, in order
    #[serde(rename = "commit")]
    pub commits: Vec<CommitSpec>,
}

/// A single logical commit to reconstruct.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitSpec {
    /// The commit message (first line)
    pub message: String,

    /// Guidance for the LLM on what changes belong in this commit
    #[serde(default)]
    pub hints: Option<String>,

    /// Execution history - herodotus appends entries as it works
    #[serde(default)]
    pub history: Vec<HistoryEntry>,
}

/// An entry in a commit's execution history.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryEntry {
    /// A commit was created (main or WIP fix)
    CommitCreated(String),

    /// LLM assessed it cannot proceed - needs human intervention
    Stuck(String),

    /// Human resolved a stuck state - describes what changed
    Resolved(String),

    /// This logical commit is done
    Complete,
}

impl HistorySpec {
    /// Parse a history spec from TOML content.
    pub fn from_toml(content: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(content)
    }

    /// Serialize the spec back to TOML.
    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }

    /// Find the index of the first commit that isn't complete.
    ///
    /// Returns `None` if all commits are complete.
    pub fn next_pending_commit(&self) -> Option<usize> {
        self.commits.iter().position(|c| !c.is_complete())
    }
}

impl CommitSpec {
    /// Check if this commit is complete.
    pub fn is_complete(&self) -> bool {
        matches!(self.history.last(), Some(HistoryEntry::Complete))
    }

    /// Check if this commit is stuck and awaiting human resolution.
    ///
    /// Returns `true` if the last entry is `Stuck`. Returns `false` if
    /// the human has added a `Resolved` entry after the `Stuck`.
    pub fn is_stuck(&self) -> bool {
        matches!(self.history.last(), Some(HistoryEntry::Stuck(_)))
    }

    /// Check if this commit was stuck but has been resolved by a human.
    ///
    /// Returns `true` if the last entry is `Resolved`.
    pub fn is_resolved(&self) -> bool {
        matches!(self.history.last(), Some(HistoryEntry::Resolved(_)))
    }

    /// Get the resolution note if this commit was resolved.
    pub fn resolution_note(&self) -> Option<&str> {
        match self.history.last() {
            Some(HistoryEntry::Resolved(note)) => Some(note),
            _ => None,
        }
    }
}
