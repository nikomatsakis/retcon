# Creating a Retcon History Specification

## Overview

This SOP guides you through creating a history specification for retcon, a tool that reconstructs clean git history from messy branches. The goal is to create a TOML specification that describes logical commits to reconstruct.

## Parameters

- **source_branch** (required): The branch with the user's changes (the messy history)
- **target_branch** (required): The branch this will merge into (e.g., `origin/main`)

**Constraints for parameter acquisition:**
- You MUST ask for both required parameters upfront in a single prompt
- You MUST NOT proceed until both parameters are provided

## Steps

### 1. Gather Branch Information

Ask the user for the source and target branches, then establish the working context.

**Constraints:**
- You MUST ask for source_branch and target_branch in a single prompt
- You MUST propose a name for the clean branch (typically `<source>-clean`) and confirm with the user
- You MUST find the merge-base: `git merge-base <source> <target>`
- You MUST verify the merge-base exists before proceeding
- You SHOULD inform the user of the merge-base commit

### 2. Analyze the Diff

Examine the full diff from merge-base to source to understand what changed.

**Constraints:**
- You MUST examine the diff, e.g. by executing `git diff $(git merge-base <source> <target>)..<source>`
- You SHOULD examine file statistics: `git diff $(git merge-base <source> <target>)..<source> --stat`
- You MAY examine commit history for context: `git log $(git merge-base <source> <target>)..<source> --oneline`
- You MUST focus on the **diff**, not the commit history - the diff shows what actually changed; the commits show how they got there (which is what we're cleaning up)

### 3. Identify Development Strands

Analyze the diff to identify distinct logical groupings of changes.

**Constraints:**
- You MUST identify the main "strands" of work:
  - Refactorings (code movement, renames, reorganization - no behavior change)
  - New features (new functionality being added)
  - Bug fixes (corrections to existing behavior)
  - Cleanups (style changes, removing dead code, dependency updates)
  - Tests (new or modified tests)
- You MUST note which files belong to which strand
- You MUST identify files that have changes from multiple strands - these need careful handling in hints
- You SHOULD present a summary of identified strands to the user

### 4. Propose Commit Structure

Design a commit sequence that tells a clear story, following these default principles:

**Default Principles:**
- Each commit should be a conceptual layer that builds toward the final picture
- Separate refactorings (no behavior change) from behavior-defining changes
- One logical change per commit - a commit should do one thing well
- Refactors before features - extract/reorganize code before adding new functionality
- Order matters - dependencies come first
- Tests with implementation unless it's a pure test addition
- Avoid commits that modify the same code as earlier commits - but extending logic with additional details is fine (e.g., adding cases to a match, adding fields to a struct)

**Constraints:**
- You MUST organize commits to follow the default principles unless the user specifies different priorities
- You MUST present the proposed commit structure to the user for feedback
- You MUST allow the user to request changes:
  - Reordering commits
  - Splitting commits into smaller pieces
  - Combining commits
  - Adding or removing commits
- You MUST iterate on the proposal until the user approves
- You SHOULD use conventional commit message style:
  - `feat:` new feature
  - `fix:` bug fix
  - `refactor:` code reorganization without behavior change
  - `test:` adding or updating tests
  - `docs:` documentation changes
  - `chore:` maintenance tasks

### 5. Write the TOML Specification

Once the user approves the commit structure, write out the retcon specification file.

**Constraints:**
- You MUST write the specification to a file (suggest `retcon-spec.toml` or let user specify)
- You MUST use the following format:

```toml
source = "feature-branch"           # Branch with your changes
remote = "origin/main"              # Target branch for merge
cleaned = "feature-branch-clean"    # New branch to create

[[commit]]
message = "type: concise description of the change"
hints = """
Detailed guidance for extracting this commit:
- Which files to modify
- What changes belong here vs other commits
- Dependencies on previous commits
- Tricky areas to watch for
"""

[[commit]]
message = "next logical commit"
hints = """
...
"""
```

- You MUST include specific hints for each commit:
  - Name specific files, functions, and modules
  - Note what to exclude (changes that belong to other commits)
  - Note dependencies on previous commits
  - Call out tricky areas
- You MUST NOT use vague hints - specificity is critical for successful reconstruction
- You MUST confirm the file was written successfully

### 6. Provide Next Steps

After writing the specification, instruct the user on how to execute it.

**Constraints:**
- You MUST tell the user to run: `retcon execute <spec-file>`
- You SHOULD remind them that retcon will:
  - Create the clean branch from the merge-base
  - Reconstruct each commit in order
  - Run build verification after each commit
  - Stop and ask for help if stuck

## Example

Given this messy history:
```
a1b2c3 WIP oauth stuff
d4e5f6 fix compile errors
g7h8i9 more oauth work
j0k1l2 extract validation (should have done this first)
m3n4o5 fix tests
p6q7r8 actually fix oauth token refresh
s9t0u1 remove debug logging
```

A good reconstruction might be:

```toml
source = "feature-oauth"
remote = "origin/main"
cleaned = "feature-oauth-clean"

[[commit]]
message = "refactor: extract validation logic to dedicated module"
hints = """
Move validate_user() and validate_session() from lib.rs to validation.rs.
Include the ValidationError enum.
Pure reorganization - no behavior changes.
Do NOT include any OAuth-related changes.
"""

[[commit]]
message = "feat: add OAuth provider authentication"
hints = """
New oauth.rs module with OAuthProvider trait.
Includes Google and GitHub implementations.
Token refresh logic in token.rs.
Exclude debug logging (removed in final state anyway).
"""

[[commit]]
message = "test: update tests for OAuth support"
hints = """
Test updates in tests/auth_test.rs.
New mock provider for testing.
"""
```

The messy "how it happened" becomes a clean "conceptual layers" story.
