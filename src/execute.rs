//! Execute the history reconstruction loop.

use std::path::Path;
use std::process::Command;

use determinishtic::Determinishtic;
use sacp_tokio::AcpAgent;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::git::Git;
use crate::spec::{HistoryEntry, HistorySpec};

/// Execute the reconstruction loop for the given spec file.
///
/// This reads the spec, finds the next pending commit, and uses the LLM
/// to reconstruct it. Progress is saved back to the spec file after each
/// commit attempt.
pub async fn execute(spec_path: &Path) -> Result<(), Error> {
    let content = std::fs::read_to_string(spec_path).map_err(|e| Error::ReadSpec {
        path: spec_path.display().to_string(),
        source: e,
    })?;
    let mut spec = HistorySpec::from_toml(&content)?;

    // Find where to resume
    let Some(start_idx) = spec.next_pending_commit() else {
        println!("All commits are complete!");
        return Ok(());
    };

    println!(
        "Resuming from commit {}/{}: {}",
        start_idx + 1,
        spec.commits.len(),
        spec.commits[start_idx].message
    );

    // Find the git repository root
    let git = Git::discover(spec_path)?;

    // Set up git state: create cleaned branch from merge-base if it doesn't exist
    setup_cleaned_branch(&git, &spec)?;

    // Connect to the LLM agent
    println!("Connecting to LLM agent...");
    let agent = AcpAgent::zed_claude_code();
    let d = Determinishtic::new(agent)
        .await
        .map_err(|e| Error::AgentConnect { source: e.into() })?;
    println!("Connected.");

    // Process each pending commit
    for commit_idx in start_idx..spec.commits.len() {
        let commit_spec = &spec.commits[commit_idx];
        println!(
            "\nCommit {}/{}: {}",
            commit_idx + 1,
            spec.commits.len(),
            commit_spec.message
        );

        // Check if stuck from previous run - require human resolution
        if commit_spec.is_stuck() {
            println!("  ✗ Previously stuck - add a `resolved` entry to continue");
            println!("    Edit the spec file and add after the `stuck` entry:");
            println!("    {{ resolved = \"description of what you changed\" }}");
            return Ok(());
        }

        // If resolved, include the resolution note in context
        let resolution_note = commit_spec.resolution_note();
        if let Some(note) = resolution_note {
            println!("  Resolved: {note}");
        }

        // Run the reconstruction for this commit
        let result = reconstruct_commit(&d, &git, &spec, commit_idx, resolution_note).await;

        match result {
            Ok(entries) => {
                // Append history entries and save
                spec.commits[commit_idx].history.extend(entries);
                save_spec(spec_path, &spec)?;

                if spec.commits[commit_idx].is_complete() {
                    println!("  ✓ Commit complete");
                } else if spec.commits[commit_idx].is_stuck() {
                    println!("  ✗ Stuck - stopping");
                    return Ok(());
                }
            }
            Err(e) => {
                // Record the error and stop
                spec.commits[commit_idx]
                    .history
                    .push(HistoryEntry::Stuck(e.to_string()));
                save_spec(spec_path, &spec)?;
                return Err(e);
            }
        }
    }

    println!("\nAll specified commits reconstructed.");

    // Catchall phase: ensure cleaned branch matches source exactly
    finalize_remaining_changes(&d, &git, &spec).await?;

    println!("\nComplete! Reconstructed branch matches source.");
    Ok(())
}

/// Reconstruct a single commit, returning history entries to append.
async fn reconstruct_commit(
    d: &Determinishtic,
    git: &Git,
    spec: &HistorySpec,
    commit_idx: usize,
    resolution_note: Option<&str>,
) -> Result<Vec<HistoryEntry>, Error> {
    let commit_spec = &spec.commits[commit_idx];
    let mut entries = Vec::new();

    // Get the diff from cleaned to source
    let diff = git.diff(&spec.cleaned, &spec.source)?;
    if diff.is_empty() {
        // No more changes to extract
        entries.push(HistoryEntry::Complete);
        return Ok(entries);
    }

    // Build the prompt for extracting this commit
    let hints = commit_spec.hints.as_deref().unwrap_or("No specific hints");

    // Build resolution context if this is a retry after human intervention
    let resolution_context = resolution_note
        .map(|note| {
            format!(
                "\n## Previous attempt was stuck - human provided resolution:\n{note}\n\nUse this context to guide your approach.\n"
            )
        })
        .unwrap_or_default();

    // First pass: extract and apply changes
    let extract_result: ExtractResult = d
        .think()
        .textln("# Task: Extract changes for a git commit")
        .textln("")
        .textln("You are reconstructing clean git history from a messy branch.")
        .textln("Your job is to extract ONLY the changes relevant to this commit.")
        .text(&resolution_context)
        .textln("")
        .textln("## Commit to create:")
        .textln(&format!("Message: {}", commit_spec.message))
        .textln(&format!("Hints: {hints}"))
        .textln("")
        .textln("## Available diff (cleaned..source):")
        .textln("```diff")
        .text(&diff)
        .textln("```")
        .textln("")
        .textln("## Instructions:")
        .textln("1. Examine current file contents if needed")
        .textln("2. Write the relevant changes from the diff to the appropriate files")
        .textln("3. Only include changes that belong to THIS commit based on the message and hints")
        .textln("4. Leave other changes for subsequent commits")
        .textln("")
        .textln("When done, return whether you successfully applied changes.")
        .await
        .map_err(|e| Error::Agent {
            message: e.to_string(),
        })?;

    if !extract_result.applied_changes {
        entries.push(HistoryEntry::Stuck(
            "LLM could not extract changes".to_string(),
        ));
        return Ok(entries);
    }

    // Create the commit
    let hash = git.commit(&commit_spec.message)?;
    entries.push(HistoryEntry::CommitCreated(hash));
    println!("  Created commit");

    // Enter the fix loop
    loop {
        // Run build
        println!("  Building...");
        let build_result = run_build(git.root())?;

        if build_result.success {
            println!("  Build passed");
            entries.push(HistoryEntry::Complete);
            return Ok(entries);
        }

        println!("  Build failed, consulting LLM...");

        // Get fresh diff - maybe we need to pull more from source
        let fresh_diff = git.diff(&spec.cleaned, &spec.source)?;

        // Ask LLM if it can make progress
        let assess_result: AssessResult = d
            .think()
            .textln("# Task: Fix build failure or report stuck")
            .textln("")
            .textln("The build failed after applying changes. You need to either fix it or report that you're stuck.")
            .textln("")
            .textln("## Build output:")
            .textln("```")
            .text(&build_result.output)
            .textln("```")
            .textln("")
            .textln("## Remaining diff (cleaned..source):")
            .textln("```diff")
            .text(&fresh_diff)
            .textln("```")
            .textln("")
            .textln("## Original commit:")
            .textln(&format!("Message: {}", commit_spec.message))
            .textln(&format!("Hints: {hints}"))
            .textln("")
            .textln("## Instructions:")
            .textln("1. Analyze the build error")
            .textln("2. Check if additional changes from the diff would fix it")
            .textln("3. If you can fix it: write the fixes to the appropriate files")
            .textln("4. If you're stuck (circular dependency, missing context, etc): report why")
            .textln("")
            .textln("Return can_progress=true if you applied fixes, false if stuck.")
            .await
            .map_err(|e| Error::Agent {
                message: e.to_string(),
            })?;

        if !assess_result.can_progress {
            // LLM is stuck
            let reason = assess_result
                .stuck_reason
                .unwrap_or_else(|| "Unknown reason".to_string());
            entries.push(HistoryEntry::Stuck(reason));
            return Ok(entries);
        }

        // LLM made fixes, create a WIP commit
        let wip_message = format!("WIP: fix for {}", commit_spec.message);
        let hash = git.commit(&wip_message)?;
        entries.push(HistoryEntry::CommitCreated(hash));
        println!("  Created WIP commit");

        // Loop continues to re-run build
    }
}

/// Finalize any remaining changes that weren't captured by the specified commits.
///
/// This ensures the invariant: cleaned branch must match source branch exactly.
async fn finalize_remaining_changes(
    d: &Determinishtic,
    git: &Git,
    spec: &HistorySpec,
) -> Result<(), Error> {
    // Check if there's any remaining diff
    let diff = git.diff(&spec.cleaned, &spec.source)?;
    if diff.is_empty() {
        return Ok(());
    }

    println!("\nRemaining changes detected - creating WIP commits...");

    // Build a summary of commits that were created
    let commit_summary: String = spec
        .commits
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{}. {}", i + 1, c.message))
        .collect::<Vec<_>>()
        .join("\n");

    // We need the root path for the tool closure
    let repo_root = git.root().to_path_buf();
    let commits = spec.commits.clone();
    let source = spec.source.clone();

    // Ask LLM to analyze and create WIP commits
    let _result: CatchallResult = d
        .think()
        .textln("# Task: Create WIP commits for remaining changes")
        .textln("")
        .textln("The main reconstruction is complete, but some changes were missed.")
        .textln("Your job is to apply ALL remaining changes, creating WIP commits that indicate")
        .textln("which original commit they should be merged into during interactive rebase.")
        .textln("")
        .textln("## Commits that were created:")
        .textln(&commit_summary)
        .textln("")
        .textln("## Remaining diff (cleaned..source):")
        .textln("```diff")
        .text(&diff)
        .textln("```")
        .textln("")
        .textln("## Instructions:")
        .textln("1. Analyze which original commit each change logically belongs to")
        .textln("2. Group changes by target commit")
        .textln("3. For each group, write the changes to the appropriate files")
        .textln("4. After each group, call create_wip_commit with the target commit number")
        .textln("5. Apply ALL changes from the diff - don't leave anything out")
        .textln("")
        .textln("The WIP commits will be named 'WIP--merge into <N>: <original message>'")
        .textln("so the user knows where to squash them during rebase -i.")
        .define_tool(
            "create_wip_commit",
            "Create a WIP commit for changes that belong to a specific original commit",
            {
                let repo = repo_root.clone();
                let commits = commits.clone();
                async move |input: CreateWipCommitInput, _cx| {
                    let target_idx = input.target_commit_number.saturating_sub(1);
                    let target_message = commits
                        .get(target_idx)
                        .map_or("unknown", |c| c.message.as_str());

                    let wip_message =
                        format!("WIP--merge into {}: {}", input.target_commit_number, target_message);

                    // Stage and commit
                    let status = Command::new("git")
                        .args(["add", "-A"])
                        .current_dir(&repo)
                        .status();

                    if status.map(|s| !s.success()).unwrap_or(true) {
                        return Ok(CreateWipCommitOutput {
                            error: Some("Failed to stage changes".to_string()),
                        });
                    }

                    let status = Command::new("git")
                        .args(["commit", "-m", &wip_message])
                        .current_dir(&repo)
                        .status();

                    if status.map(|s| !s.success()).unwrap_or(true) {
                        return Ok(CreateWipCommitOutput {
                            error: Some("Failed to create commit".to_string()),
                        });
                    }

                    println!("  Created: {wip_message}");
                    Ok(CreateWipCommitOutput { error: None })
                }
            },
            sacp::tool_fn_mut!(),
        )
        .await
        .map_err(|e| Error::Agent {
            message: e.to_string(),
        })?;

    // Check if there's still a diff after LLM's attempt
    let remaining_diff = git.diff(&spec.cleaned, &spec.source)?;
    if remaining_diff.is_empty() {
        return Ok(());
    }

    // Still have remaining changes - create a final catchall commit
    println!("\n⚠ Some changes still remain - creating catchall commit...");

    // Apply all remaining changes by checking out files from source
    git.checkout_files(&source, ".")?;
    let _hash = git.commit("WIP--remaining changes (review manually)")?;

    println!("  Created: WIP--remaining changes (review manually)");
    println!("\n⚠ Warning: Some changes could not be automatically categorized.");
    println!("  Please review the 'WIP--remaining changes' commit and distribute");
    println!("  its contents to the appropriate commits during interactive rebase.");

    Ok(())
}

// =============================================================================
// Tool Input/Output Types
// =============================================================================

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct ExtractResult {
    /// Whether changes were successfully applied
    applied_changes: bool,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct AssessResult {
    /// Whether the LLM can make progress on fixing the build
    can_progress: bool,
    /// If stuck, explanation of why
    stuck_reason: Option<String>,
}

#[derive(Debug)]
struct BuildResult {
    success: bool,
    output: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct CreateWipCommitInput {
    /// Which commit number (1-indexed) this change belongs to
    target_commit_number: usize,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct CreateWipCommitOutput {
    /// Error message if the commit failed
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct CatchallResult {
    /// Number of WIP commits created
    commits_created: usize,
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Set up the cleaned branch from merge-base if it doesn't exist.
fn setup_cleaned_branch(git: &Git, spec: &HistorySpec) -> Result<(), Error> {
    if git.ref_exists(&spec.cleaned) {
        git.checkout(&spec.cleaned)?;
        println!("Checked out existing branch: {}", spec.cleaned);
    } else {
        let base = git.merge_base(&spec.source, &spec.remote)?;
        git.checkout_new_branch(&spec.cleaned, &base)?;
        println!(
            "Created branch {} from merge-base {}",
            spec.cleaned,
            &base[..8.min(base.len())]
        );
    }
    Ok(())
}

/// Run the build command.
fn run_build(repo_root: &Path) -> Result<BuildResult, Error> {
    // TODO: Make build command configurable in spec
    let output = Command::new("cargo")
        .args(["check"])
        .current_dir(repo_root)
        .output()
        .map_err(|e| Error::Build(format!("failed to run cargo check: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    Ok(BuildResult {
        success: output.status.success(),
        output: format!("{stdout}\n{stderr}"),
    })
}

/// Save the spec back to the TOML file.
fn save_spec(spec_path: &Path, spec: &HistorySpec) -> Result<(), Error> {
    let content = spec.to_toml()?;
    std::fs::write(spec_path, &content).map_err(|e| Error::WriteSpec {
        path: spec_path.display().to_string(),
        source: e,
    })?;
    Ok(())
}

// =============================================================================
// Error Types
// =============================================================================

/// Errors that can occur during execution.
#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to read spec file '{path}'")]
    ReadSpec {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse spec file")]
    ParseSpec(#[from] toml::de::Error),

    #[error("failed to serialize spec")]
    SerializeSpec(#[from] toml::ser::Error),

    #[error("failed to write spec to '{path}'")]
    WriteSpec {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("git: {0}")]
    Git(#[from] crate::git::Error),

    #[error("build: {0}")]
    Build(String),

    #[error("failed to connect to LLM agent")]
    AgentConnect {
        #[source]
        source: anyhow::Error,
    },

    #[error("LLM agent error: {message}")]
    Agent { message: String },
}
