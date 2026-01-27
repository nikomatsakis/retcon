# Git History Cleanup for PR Preparation

## Overview

This script helps transform a messy git branch history into a clear, reviewable story suitable for pull requests. The goal is to create commits that act as conceptual layers, where each commit is understandable on its own and builds toward the final picture.

## Parameters

- **pr_description** (required): A brief description of what this PR accomplishes overall. This provides context for how individual commits should fit into the larger narrative.
- **target_branch** (required): The branch this PR will be merged into (e.g., `origin/main`, `origin/develop`). This determines the base for commit analysis and cleanup.

**Constraints for parameter acquisition:**
- You MUST ask for all required parameters upfront in a single prompt rather than one at a time
- You MUST support multiple input methods including:
  - Direct input: Text provided directly in the conversation
  - File path: Path to a local file containing the PR description
  - URL: Link to an internal resource
  - Other methods: You SHOULD be open to other ways the user might want to provide the data
- You MUST use appropriate tools to access content based on the input method
- You MUST confirm successful acquisition of all parameters before proceeding
- You SHOULD save the PR description to a consistent location for reference during the cleanup process

## Steps

### 1. Establish and Confirm Cleanup Principles

Present the suggested principles for guiding the cleanup process and verify they align with the user's goals.

**Suggested Commit Structure Principles:**
- Each commit should be a conceptual layer that builds toward the final picture
- Separate refactorings (no behavior change) from behavior-defining changes
- Minimize commits that modify the same code repeatedly (different parts of the same file is fine)
- Prioritize: conceptual clarity (layers) > independent buildability > test passage

**Suggested Commit Message Principles:**
- Explain briefly what the commit does
- More importantly, explain how it fits into the overall PR narrative
- Indicate if this lays groundwork for future changes
- Note dependencies on future work when relevant

**Constraints:**
- You MUST present these principles as suggestions, not requirements
- You MUST ask the user if they agree with these principles or have different priorities
- You MUST allow the user to modify, add, or remove principles based on their context
- You MUST confirm the final set of principles before proceeding with any git operations
- You SHOULD provide examples of how different principles would affect commit organization
- You MUST NOT proceed until the user explicitly agrees to the cleanup approach

### 2. Create Clean Branch and Extract Changes

Create a clean working branch and extract all changes as a diff file for safe incremental application.

**Constraints:**
- You MUST verify the working directory is clean (no unstaged or staged changes) before proceeding
- You MUST create a backup branch named `backup-[original-branch-name]-[timestamp]`
- You MUST verify the backup was created successfully before proceeding
- You MUST create a clean working branch named `[original-branch-name]-[date]-clean` based on the target branch
- You MUST generate a complete diff file: `git diff [clean-branch] [original-branch] > /tmp/HISTORY_FULL_CHANGES.diff`
- You MUST verify the diff file contains all expected changes
- You MUST switch to the clean branch for all subsequent work
- You MUST inform the user of both branch names and explain the diff-based approach

### 3. Analyze Changes and Development Story

Examine the full-changes.diff and any helpful context from original commits to understand the development story and identify cleanup opportunities.

**Constraints:**
- You MUST analyze the `/tmp/HISTORY_FULL_CHANGES.diff` file to understand what actually changed
- You SHOULD examine original commit history for context about intent and development strands
- You MUST identify the main "strands" of development by analyzing the diff:
  - What are the major pieces and how do they interact?
  - Which changes are refactorings vs behavior changes vs orthogonal cleanups?
  - Which files/functions are modified and for what purposes?
- You MUST organize changes by logical purpose rather than by original commit sequence
- You SHOULD read original commit messages when they provide useful context about intent
- You MUST focus on the end state (what the diff accomplishes) rather than the development path
- You SHOULD present a summary of development strands based on the diff analysis and ask if your analysis matches the user's understanding

### 4. Develop Commit Plan

Create a concrete proposal for the final commit sequence that best meets the established principles, stored as an executable refactoring plan.

Using the development strands identified in step 3, develop a commit plan with the following structure:

**Constraints:**
- You MUST create a `REFACTORING_PLAN.md` file in the repository root directory with specific format for recursive execution
- You MUST include an "Agent instructions" section that defines how to process each commit
- You MUST include an "Overview" section with a checklist of commits and their main messages
- You MUST include a "Details" section with full specifications for each commit
- You MUST organize commits to follow the agreed-upon principles (e.g., refactorings before behavior changes)
- You MUST ensure each proposed commit has a clear, single purpose within the overall story
- For each commit detail, You MUST specify:
  - The complete commit message
  - Specific list of files containing relevant changes
  - For files with changes from multiple strands, precise guidance on which changes belong to this commit (including diff hunks if necessary)
  - Which existing commits or parts of commits are involved
  - Concrete instructions that put you in the shoes of the executing agent
- You MUST design the plan so recursive agent invocation can process one commit at a time
- You MUST iterate on the `REFACTORING_PLAN.md` file with the user until they approve it
- You MUST allow the user to edit the plan directly or request modifications
- You MUST NOT proceed to execution until the user explicitly approves the final plan

### 5. Execute Refactoring Plan

Execute the approved refactoring plan through collaborative delegation using the diff-based approach.

**Constraints:**
- You MUST ensure you are working on the clean branch created in step 2
- You MUST have the `/tmp/HISTORY_FULL_CHANGES.diff` file available as the source of all changes
- You MUST execute the plan iteratively using delegation:
  - For each uncompleted item in the REFACTORING_PLAN.md checklist:
    - Create a `/tmp/HISTORY_DELEGATED_TASK.md` file with the specific commit task
    - Recommend running: `kiro-cli chat --non-interactive --trust-all-tools "$(cat /tmp/HISTORY_DELEGATED_TASK.md)" 2>&1 | tee /tmp/HISTORY_DELEGATE.log`
    - Wait for the delegated agent to complete and report results in the file
    - Review the results and update the main plan accordingly
  - Continue until all commits are completed or plan revision is needed
- You MUST monitor for execution issues and adapt the plan:
  - If a delegated agent reports BLOCKED or NEED_HELP, revise the plan
  - If dependencies are discovered, reorder or split commits as needed
  - If the current approach isn't working, consider alternative strategies
- You MUST verify the final git history matches the approved plan (as revised)
- You MUST verify that the final state matches the original branch by comparing against `/tmp/HISTORY_FULL_CHANGES.diff`
- You MUST provide instructions for pushing the cleaned history when complete

## File Formats

### REFACTORING_PLAN.md Format

The refactoring plan file MUST follow this format:

```markdown
# Git History Refactoring Plan

## Overview

This plan organizes the git history cleanup for: [PR description]

**Commits to create:**
* [ ] Rename wiz to bang
* [ ] Add hook for customizing bang message  
* [ ] Distinguish bang messages based on context

## Commit Details

### 1. Rename wiz to bang

**Complete commit message:**
```
refactor: rename wiz to bang for clarity

Renames the core wiz functionality to bang to better reflect its purpose
in the overall authentication system. This prepares for the upcoming hook
system by making the naming more intuitive.
```

**Diff sections to include:**
- **Lines 15-20 in full-changes.diff**: `WIZ_CONFIG` → `BANG_CONFIG` rename in src/config.js
- **Lines 45-60 in full-changes.diff**: Function rename `validateWiz` → `validateBang` and exports in src/utils/validation.js
- **Exclude**: Do not include the hook mechanism changes (lines 80-95, that belongs to commit 2)

### 2. Add hook for customizing bang message

[Additional commit specifications...]
```

### DELEGATED_TASK.md Format

For each commit, create a delegation file following this format:

```markdown
# Delegated Task: [Commit Name]

## Context

**Overall PR Goal:** [Brief description from PR context]

**Current Step:** [Which step in the overall plan]

**Previous Commits:** [What has been committed so far]

## Your Mission

Create one commit that accomplishes: [Specific commit goal]

**Target commit message:**
```
[Full commit message as specified in plan]
```

## Current State

**Available files:**
- `/tmp/HISTORY_FULL_CHANGES.diff` - Complete diff of all changes from original branch
- Current working directory on clean branch `[clean-branch-name]`

**Your task:** Copy relevant sections from `/tmp/HISTORY_FULL_CHANGES.diff` to `/tmp/HISTORY_TO_STAGE.diff`, apply, and commit

## Expected Changes

**Diff sections to copy:** [Specific line ranges and sections from `/tmp/HISTORY_FULL_CHANGES.diff`]

**Files that should be modified:** [List of files that will change]

## Process

1. **FIRST**: Regenerate the diff to ensure it's current: `git diff [clean-branch-name] [original-branch-name] > /tmp/HISTORY_FULL_CHANGES.diff`
2. Examine `/tmp/HISTORY_FULL_CHANGES.diff` to understand all remaining changes
3. Copy the specified sections from `/tmp/HISTORY_FULL_CHANGES.diff` to `/tmp/HISTORY_TO_STAGE.diff`
4. Apply the patch: `git apply /tmp/HISTORY_TO_STAGE.diff` (or `git apply /tmp/HISTORY_TO_STAGE.diff -- specific/file.js` for large diffs)
5. Verify the changes look correct with `git status` and `git diff`
6. Make small compilation fixes if needed for this commit to build independently
7. Create the commit with the specified message
8. Test that the commit builds correctly (run build/test commands if available)
9. Clean up `/tmp/HISTORY_TO_STAGE.diff` file
10. Report results below

## Troubleshooting

- **For large diffs**: Focus on copying only the sections for files you're actually modifying in this commit
- If `git apply` fails, check that you copied the diff sections correctly and ensure proper formatting
- If you discover unexpected dependencies, report them rather than forcing the commit
- If the specified diff sections don't make sense together, suggest alternative groupings
- If build/compilation fails, try copying additional related sections from `/tmp/HISTORY_FULL_CHANGES.diff`
- When in doubt, ask for help rather than making assumptions

## RESULTS

**Status:** [SUCCESS/NEED_HELP/BLOCKED - fill this in when complete]

**Commits Made:** [List any commits created]

**Issues Encountered:** [Describe any problems]

**Remaining Staged Changes:** [What's left for future commits]

**Suggested Plan Changes:** [Any recommendations for revising the plan]
```

### 6. Final Review and Verification

Present the cleaned-up history and verify it meets the storytelling goals.

**Constraints:**
- You MUST show the final commit history with new messages
- You MUST verify that the story flows logically from commit to commit
- You SHOULD check that refactorings come before related behavior changes
- You MUST confirm that the final state matches the original branch tip
- You SHOULD suggest any remaining improvements to commit organization
- You MUST provide instructions for pushing the cleaned history
- You MUST ask the user what they want to do with the temporary files:
  - `REFACTORING_PLAN.md` (the overall plan)
  - `/tmp/HISTORY_DELEGATED_TASK.md` (the final delegation file)
  - `/tmp/HISTORY_FULL_CHANGES.diff` (the complete diff file)
  - `/tmp/HISTORY_DELEGATE.log` (delegation execution log)
  - Clean branch `[original-branch-name]-[date]-clean`
- You MUST NOT commit these planning and diff files to the repository

## Examples

### Example PR Description
```
This PR adds OAuth 2.0 authentication to replace our basic username/password system. It maintains backward compatibility during a transition period and includes proper session management.
```

### Example of Good Commit Sequence
```
1. Extract user validation logic (prepare for OAuth integration)
2. Add OAuth configuration and dependencies
3. Implement OAuth provider authentication flow
4. Add backward compatibility layer for existing auth
5. Update session management for OAuth tokens
6. Add OAuth-specific error handling and logging
```

### Example of Poor Commit Sequence (to be cleaned up)
```
1. Add OAuth stuff
2. Fix compilation errors
3. More OAuth work
4. Fix tests
5. Actually fix the OAuth implementation
6. Remove debug logging
7. Fix merge conflicts
```

## Troubleshooting

### Rebase Conflicts
If conflicts occur during rebase operations, you MUST:
- Pause the rebase process
- Explain the conflict to the user
- Guide them through resolution
- Verify the resolution maintains the intended story flow

### Lost Work Recovery
If operations fail and work appears lost:
- Direct the user to the backup branch created in step 2
- Provide commands to restore from backup
- Suggest starting over with smaller, incremental changes