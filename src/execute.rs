//! Execute the history reconstruction loop.

use std::path::Path;

use crate::spec::HistorySpec;

/// Execute the reconstruction loop for the given spec file.
///
/// This reads the spec, finds the next pending commit, and uses the LLM
/// to reconstruct it. Progress is saved back to the spec file after each
/// commit attempt.
pub async fn execute(spec_path: &Path) -> Result<(), Error> {
    let content = std::fs::read_to_string(spec_path).map_err(Error::ReadSpec)?;
    let spec = HistorySpec::from_toml(&content).map_err(Error::ParseSpec)?;

    // Find where to resume
    let Some(next_idx) = spec.next_pending_commit() else {
        println!("All commits are complete!");
        return Ok(());
    };

    println!(
        "Resuming from commit {}/{}: {}",
        next_idx + 1,
        spec.commits.len(),
        spec.commits[next_idx].message
    );

    // TODO: Implement the reconstruction loop with determinishtic
    // - Set up git worktree or checkout cleaned branch
    // - For each pending commit:
    //   - Give LLM the diff, hints, and tools
    //   - Let it extract and apply changes
    //   - Run build/tests
    //   - If pass: commit and mark complete
    //   - If fail: LLM assesses if it can fix or is stuck
    // - Save progress to spec file after each attempt

    todo!("implement reconstruction loop")
}

/// Errors that can occur during execution.
#[derive(Debug)]
pub enum Error {
    ReadSpec(std::io::Error),
    ParseSpec(toml::de::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::ReadSpec(e) => write!(f, "failed to read spec file: {e}"),
            Error::ParseSpec(e) => write!(f, "failed to parse spec file: {e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::ReadSpec(e) => Some(e),
            Error::ParseSpec(e) => Some(e),
        }
    }
}
