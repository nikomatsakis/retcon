//! Execute the history reconstruction loop.

use std::path::Path;
use std::process::Command as StdCommand;
use std::str::FromStr;

use determinishtic::Determinishtic;
use sacp::Agent;
use sacp::role::{HasPeer, Role};
use sacp_tokio::AcpAgent;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::git::Git;
use crate::spec::{CommitSpec, HistoryEntry, HistorySpec};

// =============================================================================
// Hooks Trait
// =============================================================================

/// Status of a commit in the execution plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitStatus {
    /// Not yet started
    Pending,
    /// Currently being processed
    InProgress,
    /// Successfully completed
    Completed,
    /// Stuck and needs human intervention
    Stuck,
}

/// Hooks for observing execution progress.
///
/// Implement this trait to customize how progress is reported during execution.
/// The default implementation [`PrintHooks`] prints to stdout.
///
/// The trait has two categories of methods:
/// - `report`: Receives pre-formatted status messages
/// - `plan_*`: Receives structured plan updates for UI rendering
#[allow(unused_variables)]
pub trait ExecuteHooks {
    /// Report a status message.
    ///
    /// Messages are pre-formatted and ready for display.
    fn report(&self, message: &str) {}

    /// Initialize the plan with commit messages.
    ///
    /// Called once at the start of execution with all commit messages.
    /// Implementations should store this to rebuild the plan on updates.
    fn plan_init(&self, commits: &[&str]) {}

    /// Update a commit's status in the plan.
    ///
    /// Called when a commit transitions between states.
    fn plan_update(&self, commit_idx: usize, status: CommitStatus) {}

    /// Called when the LLM gets stuck on a commit.
    ///
    /// The `reason` explains why the LLM couldn't proceed.
    /// Return `Some(response)` to auto-resolve and continue, or `None` to stop.
    fn on_stuck(&self, reason: &str) -> Option<String> {
        let _ = reason;
        None
    }
}

/// Default hooks implementation that prints to stdout.
pub struct PrintHooks;

impl ExecuteHooks for PrintHooks {
    fn report(&self, message: &str) {
        println!("{}", message);
    }

    // PrintHooks ignores plan_init/plan_update - the messages contain all needed info
}

/// No-op hooks implementation for silent execution.
pub struct NoOpHooks;

impl ExecuteHooks for NoOpHooks {}

/// Configuration for the execute command.
#[derive(Debug, Clone)]
pub struct ExecuteConfig {
    /// Build command to run after each commit. None means skip build.
    pub build_command: Option<String>,
    /// Test command to run after build passes. None means skip tests.
    pub test_command: Option<String>,
    /// Agent command string. None means use default (zed_claude_code).
    pub agent: Option<String>,
}

/// Execute the reconstruction loop for the given spec file.
///
/// This reads the spec, finds the next pending commit, and uses the LLM
/// to reconstruct it. Progress is saved back to the spec file after each
/// commit attempt.
///
/// This variant creates its own connection to the default agent (Zed Claude Code).
/// For use inside a proxy where you have an existing connection, use
/// [`execute_with_connection`] instead.
pub async fn execute(spec_path: &Path, config: &ExecuteConfig) -> Result<(), Error> {
    execute_with_hooks(spec_path, config, &PrintHooks, None).await
}

/// Execute the reconstruction loop with custom hooks and an optional observer.
///
/// This is the outer state machine loop. Each iteration:
/// 1. Reads the spec from disk (source of truth)
/// 2. Runs `execute_inner` which advances as far as it can, saving after each state change
/// 3. If stuck, prompts the user for a response
/// 4. If the user responds, appends `Resolved` to the TOML and loops
/// 5. If the user cancels (or all complete), exits
pub async fn execute_with_hooks(
    spec_path: &Path,
    config: &ExecuteConfig,
    hooks: &(impl ExecuteHooks + Sync),
    observer: Option<std::sync::Arc<dyn determinishtic::ThinkObserver>>,
) -> Result<(), Error> {
    // Connect to the LLM agent once
    hooks.report("Connecting to LLM agent...");
    let agent = match &config.agent {
        Some(cmd) => AcpAgent::from_str(cmd).map_err(|e| Error::Agent {
            message: format!("invalid agent command: {e}"),
        })?,
        None => AcpAgent::zed_claude_code(),
    };
    let mut d = Determinishtic::new(agent)
        .await
        .map_err(|e| Error::AgentConnect { source: e.into() })?;
    hooks.report("Connected.");

    if let Some(obs) = observer {
        d.set_observer(obs);
    }

    let git = Git::discover(spec_path)?;

    loop {
        // Read spec fresh from disk each iteration
        let content = std::fs::read_to_string(spec_path).map_err(|e| Error::ReadSpec {
            path: spec_path.display().to_string(),
            source: e,
        })?;
        let spec = HistorySpec::from_toml(&content)?;

        // Run one pass — this saves to disk after each state change
        let result = execute_inner(&d, spec, &git, Some(spec_path), config, hooks).await;

        // On hard error, spec was already saved by execute_inner
        let spec = match result {
            Ok(spec) => spec,
            Err((_spec, e)) => return Err(e),
        };

        // Check if we're stuck and need user input
        let stuck_commit = spec.commits.iter().position(|c| c.is_stuck());
        if let Some(idx) = stuck_commit {
            let reason = match spec.commits[idx].history.last() {
                Some(HistoryEntry::Stuck(r)) => r.as_str(),
                _ => "Unknown reason",
            };

            if let Some(response) = hooks.on_stuck(reason) {
                let mut spec = spec;
                if response == "SKIP" {
                    // Skip this commit entirely
                    spec.commits[idx].history.push(HistoryEntry::Complete);
                } else {
                    // Append Resolved to the TOML and loop
                    spec.commits[idx]
                        .history
                        .push(HistoryEntry::Resolved(response));
                }
                save_spec(spec_path, &spec)?;
                continue;
            }

            // User cancelled
            return Ok(());
        }

        // Not stuck — we're done
        return Ok(());
    }
}

/// Execute the reconstruction loop using an existing connection.
///
/// This variant accepts a `HistorySpec` directly and returns the updated spec.
/// It's designed for use inside a proxy or MCP tool where:
/// - The connection is already established
/// - The spec comes from a parameter rather than a file
/// - The caller handles persistence
///
/// # Returns
///
/// On success, returns the updated `HistorySpec`.
/// On error, returns both the (partially updated) spec and the error,
/// so the caller can still access progress made before the error.
///
/// # Example
///
/// ```rust,ignore
/// // Inside an MCP tool handler
/// async fn my_tool(cx: McpConnectionTo<Conductor>) -> Result<Output, Error> {
///     let d = Determinishtic::from_connection(cx.connection_to());
///     let spec = HistorySpec::from_toml(&toml_content)?;
///     let git = Git::discover(&repo_path)?;
///     let updated = execute_with_connection(&d, spec, &git, &config, &NoOpHooks).await;
///     // Handle result...
/// }
/// ```
pub async fn execute_with_connection<R, H>(
    d: &Determinishtic<R>,
    spec: HistorySpec,
    git: &Git,
    config: &ExecuteConfig,
    hooks: &H,
) -> Result<HistorySpec, (HistorySpec, Error)>
where
    R: Role + HasPeer<Agent>,
    H: ExecuteHooks,
{
    execute_inner(d, spec, git, None, config, hooks).await
}

/// Internal implementation shared by both execute variants.
///
/// Advances as far as it can in a single pass, saving the spec to disk
/// after each state change. Returns the final spec state.
async fn execute_inner<R, H>(
    d: &Determinishtic<R>,
    mut spec: HistorySpec,
    git: &Git,
    spec_path: Option<&Path>,
    config: &ExecuteConfig,
    hooks: &H,
) -> Result<HistorySpec, (HistorySpec, Error)>
where
    R: Role + HasPeer<Agent>,
    H: ExecuteHooks,
{
    let total = spec.commits.len();
    let verify_idx = total; // index of the "verify" entry in the plan

    // Initialize the plan: all commits + a final "verify" step
    let mut plan_messages: Vec<&str> = spec.commits.iter().map(|c| c.message.lines().next().unwrap_or("")).collect();
    plan_messages.push("Verify branch matches source");
    hooks.plan_init(&plan_messages);

    // Mark already-completed/stuck commits so the UI shows their status immediately
    for (i, commit) in spec.commits.iter().enumerate() {
        if commit.is_complete() {
            hooks.plan_update(i, CommitStatus::Completed);
        } else if commit.is_stuck() {
            hooks.plan_update(i, CommitStatus::Stuck);
        }
    }

    // Set up git state: create cleaned branch from merge-base if it doesn't exist
    setup_cleaned_branch(git, &spec, hooks).map_err(|e| (spec.clone(), e))?;

    // Find where to resume (may be None if all commits are already done)
    if let Some(start_idx) = spec.next_pending_commit() {
        hooks.report(&format!(
            "Resuming from commit {}/{}: {}",
            start_idx + 1,
            total,
            spec.commits[start_idx].message.lines().next().unwrap_or("")
        ));
    } else {
        hooks.report("All commits complete, verifying final state...");
    }

    // Process each commit
    for commit_idx in 0..spec.commits.len() {
        // Extract state from commit before any mutation
        if spec.commits[commit_idx].is_complete() {
            hooks.plan_update(commit_idx, CommitStatus::Completed);
            continue;
        }

        if spec.commits[commit_idx].is_stuck() {
            hooks.plan_update(commit_idx, CommitStatus::Stuck);
            return Ok(spec);
        }

        let was_interrupted = spec.commits[commit_idx].is_started();
        let resolution_note = spec.commits[commit_idx].resolution_note().map(String::from);

        hooks.plan_update(commit_idx, CommitStatus::InProgress);
        hooks.report(&format!(
            "\nCommit {}/{}: {}",
            commit_idx + 1,
            total,
            spec.commits[commit_idx].message.lines().next().unwrap_or("")
        ));

        if was_interrupted {
            hooks.report("  Resuming interrupted commit...");
        }

        if let Some(note) = &resolution_note {
            hooks.report(&format!("  Resolved: {note}"));
        }

        // Record Started and save before doing any work
        if !was_interrupted {
            spec.commits[commit_idx].history.push(HistoryEntry::Started);
            if let Some(p) = spec_path {
                save_spec(p, &spec).map_err(|e| (spec.clone(), e))?;
            }
        }

        // Run the reconstruction for this commit
        let result = reconstruct_commit(
            d,
            git,
            &spec,
            commit_idx,
            was_interrupted,
            resolution_note.as_deref(),
            config,
            hooks,
        )
        .await;

        match result {
            Ok(entries) => {
                spec.commits[commit_idx].history.extend(entries);
                if let Some(p) = spec_path {
                    save_spec(p, &spec).map_err(|e| (spec.clone(), e))?;
                }

                if spec.commits[commit_idx].is_complete() {
                    hooks.plan_update(commit_idx, CommitStatus::Completed);
                    hooks.report("  ✓ Commit complete");
                } else if spec.commits[commit_idx].is_stuck() {
                    hooks.plan_update(commit_idx, CommitStatus::Stuck);
                    return Ok(spec);
                }
            }
            Err(e) => {
                spec.commits[commit_idx]
                    .history
                    .push(HistoryEntry::Stuck(e.to_string()));
                if let Some(p) = spec_path {
                    let _ = save_spec(p, &spec);
                }
                hooks.plan_update(commit_idx, CommitStatus::Stuck);
                return Err((spec, e));
            }
        }
    }

    hooks.report("\nAll specified commits reconstructed.");

    // Catchall phase: ensure cleaned branch matches source exactly
    hooks.plan_update(verify_idx, CommitStatus::InProgress);
    finalize_remaining_changes(d, git, &spec, hooks)
        .await
        .map_err(|e| {
            hooks.plan_update(verify_idx, CommitStatus::Stuck);
            (spec.clone(), e)
        })?;

    hooks.plan_update(verify_idx, CommitStatus::Completed);
    hooks.report("\nComplete! Reconstructed branch matches source.");
    Ok(spec)
}

/// Reconstruct a single commit, returning history entries to append.
async fn reconstruct_commit<R, H>(
    d: &Determinishtic<R>,
    git: &Git,
    spec: &HistorySpec,
    commit_idx: usize,
    was_interrupted: bool,
    resolution_note: Option<&str>,
    config: &ExecuteConfig,
    hooks: &H,
) -> Result<Vec<HistoryEntry>, Error>
where
    R: Role + HasPeer<Agent>,
    H: ExecuteHooks,
{
    let commit_spec = &spec.commits[commit_idx];
    let mut entries = Vec::new();

    // Check if there are remaining changes
    let diff_stat = git.diff_stat(&spec.cleaned, &spec.source)?;
    if diff_stat.trim().is_empty() {
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

    // Build interrupted context if resuming after Ctrl-C
    let interrupted_context = if was_interrupted {
        "\n## Note: A previous run was interrupted.\nThere may be partial changes already in the working directory. Check the current state of files before making changes.\n"
    } else {
        ""
    };

    // First pass: extract and apply changes
    let extract_result: ExtractResult = d
        .think()
        .textln("# Task: Extract changes for a git commit")
        .textln("")
        .textln("You are reconstructing clean git history from a messy branch.")
        .textln("Your job is to extract ONLY the changes relevant to this commit.")
        .text(&resolution_context)
        .text(interrupted_context)
        .textln("")
        .textln("## Commit to create:")
        .textln(&format!("Message: {}", commit_spec.message))
        .textln(&format!("Hints: {hints}"))
        .textln("")
        .textln(&format!("## Files changed (HEAD..{}):", spec.source))
        .textln("```")
        .text(&diff_stat)
        .textln("```")
        .textln("")
        .textln(&format!(
            "To see the full diff, run: git diff HEAD {}",
            spec.source
        ))
        .textln("")
        .textln("## Instructions:")
        .textln("1. Run the git diff command above to see the available changes")
        .textln("2. Examine current file contents if needed")
        .textln("3. Write the relevant changes to the appropriate files")
        .textln("4. Only include changes that belong to THIS commit based on the message and hints")
        .textln("5. Leave other changes for subsequent commits")
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
    hooks.report("  Created commit");

    // Enter the verify/fix loop
    loop {
        // Run build if configured
        if let Some(build_cmd) = &config.build_command {
            hooks.report("  Building...");
            let build_result = run_command(git.root(), build_cmd, hooks)?;

            if !build_result.success {
                hooks.report("  Build failed, consulting LLM...");
                if !try_fix(
                    d,
                    git,
                    spec,
                    commit_spec,
                    hints,
                    &build_result,
                    &mut entries,
                    hooks,
                )
                .await?
                {
                    return Ok(entries);
                }
                // LLM made fixes, loop continues to re-verify
                continue;
            }
            hooks.report("  Build passed");
        }

        // Run tests if configured
        if let Some(test_cmd) = &config.test_command {
            hooks.report("  Testing...");
            let test_result = run_command(git.root(), test_cmd, hooks)?;

            if !test_result.success {
                hooks.report("  Tests failed, consulting LLM...");
                if !try_fix(
                    d,
                    git,
                    spec,
                    commit_spec,
                    hints,
                    &test_result,
                    &mut entries,
                    hooks,
                )
                .await?
                {
                    return Ok(entries);
                }
                // LLM made fixes, loop continues to re-verify
                continue;
            }
            hooks.report("  Tests passed");
        }

        // Both build and test passed (or were skipped)
        entries.push(HistoryEntry::Complete);
        return Ok(entries);
    }
}

/// Try to fix a build/test failure using the LLM.
/// Returns true if progress was made, false if stuck.
async fn try_fix<R, H>(
    d: &Determinishtic<R>,
    git: &Git,
    spec: &HistorySpec,
    commit_spec: &CommitSpec,
    hints: &str,
    failure: &CommandResult,
    entries: &mut Vec<HistoryEntry>,
    hooks: &H,
) -> Result<bool, Error>
where
    R: Role + HasPeer<Agent>,
    H: ExecuteHooks,
{
    // Get fresh diff stat - maybe we need to pull more from source
    let fresh_diff_stat = git.diff_stat(&spec.cleaned, &spec.source)?;

    // Ask LLM if it can make progress
    let assess_result: AssessResult = d
        .think()
        .textln("# Task: Fix build/test failure or report stuck")
        .textln("")
        .textln("The build or tests failed after applying changes. You need to either fix it or report that you're stuck.")
        .textln("")
        .textln("## Command output:")
        .textln("```")
        .text(&failure.output)
        .textln("```")
        .textln("")
        .textln(&format!("## Remaining files changed (HEAD..{}):", spec.source))
        .textln("```")
        .text(&fresh_diff_stat)
        .textln("```")
        .textln("")
        .textln(&format!("To see the full diff, run: git diff HEAD {}", spec.source))
        .textln("")
        .textln("## Original commit:")
        .textln(&format!("Message: {}", commit_spec.message))
        .textln(&format!("Hints: {hints}"))
        .textln("")
        .textln("## Instructions:")
        .textln("1. Analyze the error")
        .textln("2. Run the git diff command to check if additional changes would fix it")
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
        return Ok(false);
    }

    // LLM made fixes, create a fixup commit targeting the original
    let target_hash = entries
        .iter()
        .find_map(|e| match e {
            HistoryEntry::CommitCreated(h) => Some(h.as_str()),
            _ => None,
        })
        .unwrap_or("HEAD");
    let hash = git.commit_fixup(target_hash)?;
    entries.push(HistoryEntry::CommitCreated(hash));
    hooks.report("  Created fixup commit");

    Ok(true)
}

/// Finalize any remaining changes that weren't captured by the specified commits.
///
/// This ensures the invariant: cleaned branch must match source branch exactly.
async fn finalize_remaining_changes<R, H>(
    d: &Determinishtic<R>,
    git: &Git,
    spec: &HistorySpec,
    hooks: &H,
) -> Result<(), Error>
where
    R: Role + HasPeer<Agent>,
    H: ExecuteHooks,
{
    // Check if there's any remaining diff
    let diff_stat = git.diff_stat(&spec.cleaned, &spec.source)?;
    if diff_stat.trim().is_empty() {
        return Ok(());
    }

    hooks.report("\nRemaining changes detected - creating fixup commits...");

    // Build a summary of commits that were created
    let commit_summary: String = spec
        .commits
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{}. {}", i + 1, c.message))
        .collect::<Vec<_>>()
        .join("\n");

    // Collect the first commit hash for each logical commit (fixup target)
    let commit_hashes: Vec<Option<String>> = spec
        .commits
        .iter()
        .map(|c| {
            c.history.iter().find_map(|e| match e {
                HistoryEntry::CommitCreated(h) => Some(h.clone()),
                _ => None,
            })
        })
        .collect();

    // We need the root path for the tool closure
    let repo_root = git.root().to_path_buf();
    let source = spec.source.clone();

    // Ask LLM to analyze and create fixup commits
    let _result: CatchallResult = d
        .think()
        .textln("# Task: Create fixup commits for remaining changes")
        .textln("")
        .textln("The main reconstruction is complete, but some changes were missed.")
        .textln("Your job is to apply ALL remaining changes, creating fixup commits that")
        .textln("will be automatically squashed into the right commit during rebase --autosquash.")
        .textln("")
        .textln("## Commits that were created:")
        .textln(&commit_summary)
        .textln("")
        .textln(&format!(
            "## Remaining files changed (HEAD..{}):",
            spec.source
        ))
        .textln("```")
        .text(&diff_stat)
        .textln("```")
        .textln("")
        .textln(&format!(
            "To see the full diff, run: git diff HEAD {}",
            spec.source
        ))
        .textln("")
        .textln("## Instructions:")
        .textln("1. Run the git diff command above to see all remaining changes")
        .textln("2. Analyze which original commit each change logically belongs to")
        .textln("3. Group changes by target commit")
        .textln("4. For each group, write the changes to the appropriate files")
        .textln("5. After each group, call create_fixup_commit with the target commit number")
        .textln("6. Apply ALL changes from the diff - don't leave anything out")
        .define_tool(
            "create_fixup_commit",
            "Create a fixup commit for changes that belong to a specific original commit",
            {
                let repo = repo_root.clone();
                let commit_hashes = commit_hashes.clone();
                async move |input: CreateWipCommitInput, _cx| {
                    let target_idx = input.target_commit_number.saturating_sub(1);
                    let target_hash = commit_hashes
                        .get(target_idx)
                        .and_then(|h| h.as_deref());

                    let Some(target_hash) = target_hash else {
                        return Ok(CreateWipCommitOutput {
                            wip_message: None,
                            error: Some(format!(
                                "No commit hash found for commit {}",
                                input.target_commit_number
                            )),
                        });
                    };

                    // Stage and commit
                    let status = StdCommand::new("git")
                        .args(["add", "-A"])
                        .current_dir(&repo)
                        .status();

                    if status.map(|s| !s.success()).unwrap_or(true) {
                        return Ok(CreateWipCommitOutput {
                            wip_message: None,
                            error: Some("Failed to stage changes".to_string()),
                        });
                    }

                    let status = StdCommand::new("git")
                        .args(["commit", "--fixup", target_hash])
                        .current_dir(&repo)
                        .status();

                    if status.map(|s| !s.success()).unwrap_or(true) {
                        return Ok(CreateWipCommitOutput {
                            wip_message: None,
                            error: Some("Failed to create commit".to_string()),
                        });
                    }

                    Ok(CreateWipCommitOutput {
                        wip_message: Some(format!("fixup! {target_hash}")),
                        error: None,
                    })
                }
            },
            sacp::tool_fn_mut!(),
        )
        .await
        .map_err(|e| Error::Agent {
            message: e.to_string(),
        })?;

    // Check if there's still a diff after LLM's attempt
    let remaining_diff = git.diff_stat(&spec.cleaned, &spec.source)?;
    if remaining_diff.trim().is_empty() {
        return Ok(());
    }

    // Still have remaining changes - create a final catchall commit
    // Apply all remaining changes by checking out files from source
    git.checkout_files(&source, ".")?;
    let _hash = git.commit("WIP--remaining changes (review manually)")?;

    hooks.report("  Created: remaining uncategorized changes (review manually)");
    hooks.report("\n⚠ Warning: Some changes could not be automatically categorized.");
    hooks.report("  Review the final commit and distribute its contents using");
    hooks.report("  git rebase -i --autosquash");

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
struct CommandResult {
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
    /// The WIP commit message if successful
    wip_message: Option<String>,
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
fn setup_cleaned_branch<H: ExecuteHooks>(
    git: &Git,
    spec: &HistorySpec,
    hooks: &H,
) -> Result<(), Error> {
    if git.ref_exists(&spec.cleaned) {
        git.checkout(&spec.cleaned)?;
        hooks.report(&format!("Checked out existing branch: {}", spec.cleaned));
    } else {
        let base = git.merge_base(&spec.source, &spec.remote)?;
        git.checkout_new_branch(&spec.cleaned, &base)?;
        let base_short = &base[..8.min(base.len())];
        hooks.report(&format!(
            "Created branch {} from merge-base {}",
            spec.cleaned, base_short
        ));
    }
    Ok(())
}

/// Run a shell command, streaming output through hooks and capturing it.
fn run_command<H: ExecuteHooks>(
    repo_root: &Path,
    command: &str,
    hooks: &H,
) -> Result<CommandResult, Error> {
    use std::io::BufRead;

    // Parse command into program and args (simple shell-style splitting)
    let parts: Vec<&str> = command.split_whitespace().collect();
    let (program, args) = parts
        .split_first()
        .ok_or_else(|| Error::Command(format!("empty command: {command}")))?;

    let mut child = StdCommand::new(program)
        .args(args)
        .current_dir(repo_root)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| Error::Command(format!("failed to run '{command}': {e}")))?;

    // Read stdout and stderr in parallel using threads
    let stdout_reader = child.stdout.take().unwrap();
    let stderr_reader = child.stderr.take().unwrap();

    let stdout_handle = std::thread::spawn(move || {
        let reader = std::io::BufReader::new(stdout_reader);
        let mut lines = Vec::new();
        for line in reader.lines() {
            if let Ok(line) = line {
                lines.push(line);
            }
        }
        lines
    });

    let stderr_handle = std::thread::spawn(move || {
        let reader = std::io::BufReader::new(stderr_reader);
        let mut lines = Vec::new();
        for line in reader.lines() {
            if let Ok(line) = line {
                lines.push(line);
            }
        }
        lines
    });

    let stdout_lines = stdout_handle.join().unwrap_or_default();
    let stderr_lines = stderr_handle.join().unwrap_or_default();

    // Print all captured output through hooks
    for line in &stdout_lines {
        hooks.report(line);
    }
    for line in &stderr_lines {
        hooks.report(line);
    }

    let status = child
        .wait()
        .map_err(|e| Error::Command(format!("failed to wait for '{command}': {e}")))?;

    let mut output = stdout_lines.join("\n");
    if !stderr_lines.is_empty() {
        output.push('\n');
        output.push_str(&stderr_lines.join("\n"));
    }

    Ok(CommandResult {
        success: status.success(),
        output,
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

    #[error("command: {0}")]
    Command(String),

    #[error("failed to connect to LLM agent")]
    AgentConnect {
        #[source]
        source: anyhow::Error,
    },

    #[error("LLM agent error: {message}")]
    Agent { message: String },
}
