# User's Guide

This guide walks you through using pravda to clean up a messy git branch.

## Prerequisites

- A git repository with a messy feature branch
- The branch has been pushed or you have a backup
- `pravda` installed and in your PATH
- An LLM agent available (pravda uses Claude Code by default)

## Quick Start

```bash
# 1. Generate guidance for creating a spec
pravda prompt > guidance.md

# 2. Give the guidance to your agent to create a spec
# e.g. in `my-spec.toml`

# 3. Run the reconstruction
pravda execute my-spec.toml
```

## Step 1: Create the Spec with Your Agent

The `pravda prompt` command outputs guidance you can give to your LLM agent. The agent will analyze your diffs and propose a series of commits.

```bash
# Get the guidance prompt
pravda prompt
```

Give this prompt to your agent along with context about your branch:

```bash
# Show the agent your changes
git diff origin/main...my-feature-branch
```

The agent will:
1. Analyze the diff to understand what changed
2. Identify logical groupings (refactors, features, tests, etc.)
3. Propose a commit sequence with messages and hints

**Iterate with your agent** until you're happy with the structure. Ask questions like:
- "Can we split commit 2 into two parts?"
- "Should the config changes be in their own commit?"
- "Reorder so refactors come before features"

Once you've agreed on the structure, have the agent write out the TOML file:

```
Write this plan as a pravda spec file to my-spec.toml
```

### Manual Spec Creation

You can also create the spec manually. First understand what changed:

```bash
# See the full diff against target
git diff origin/main...my-feature-branch

# See which files changed
git diff origin/main...my-feature-branch --stat

# See the messy commit history (for context)
git log origin/main..my-feature-branch --oneline
```

Focus on the **diff**, not the commits. The diff shows what actually changed; the commits show how you got there (which is what we're cleaning up).

Then create a TOML file describing the clean history you want:

```toml
# my-spec.toml

source = "my-feature-branch"        # Your messy branch
remote = "origin/main"              # Target for the PR
cleaned = "my-feature-branch-clean" # New clean branch

[[commit]]
message = "refactor: extract validation into dedicated module"
hints = """
Move validate_user() and validate_session() from lib.rs to validation.rs.
Include the ValidationError enum.
Pure reorganization - no behavior changes.
"""

[[commit]]
message = "feat: add OAuth provider support"
hints = """
New oauth.rs module with OAuthProvider trait.
Includes Google and GitHub implementations.
Token refresh logic in token.rs.
"""

[[commit]]
message = "test: add OAuth integration tests"
hints = """
New file tests/oauth_test.rs.
Mock provider for testing.
"""
```

### Tips for Good Specs

1. **Refactors first**: Put code reorganization before new features
2. **One concept per commit**: Each commit should do one thing
3. **Specific hints**: Name files and functions, not just concepts
4. **Note exclusions**: If a file has changes for multiple commits, say which parts belong where

## Step 2: Run Pravda

```bash
pravda execute my-spec.toml
```

Pravda will:
1. Create the `cleaned` branch from the merge-base
2. For each commit in order:
   - Show the LLM the diff and your hints
   - Apply the relevant changes
   - Run `cargo check` (build verification)
   - If it passes, mark complete and move on
   - If it fails, try to fix it
   - If stuck, stop and ask for help

### Watching Progress

Pravda prints progress as it works:

```
Resuming from commit 1/3: refactor: extract validation
  Created commit
  Building...
  Build passed
  ✓ Commit complete

Commit 2/3: feat: add OAuth provider support
  Created commit
  Building...
  Build failed, consulting LLM...
  Created WIP commit
  Building...
  Build passed
  ✓ Commit complete
```

### The Spec File is State

Pravda updates your spec file as it works. After running, you'll see:

```toml
[[commit]]
message = "refactor: extract validation into dedicated module"
hints = "..."
history = [
    { commit_created = "a1b2c3d" },
    "complete",
]

[[commit]]
message = "feat: add OAuth provider support"
hints = "..."
history = [
    { commit_created = "e4f5g6h" },
    { commit_created = "i7j8k9l" },  # WIP fix
    "complete",
]
```

## Step 3: Handle Stuck States

Sometimes pravda can't proceed:

```
Commit 2/3: feat: add OAuth provider support
  ...
  Build failed, consulting LLM...
  LLM: "I'm stuck. OAuthProvider depends on SessionManager which
        isn't in the diff yet - it's probably in commit 3."
  ✗ Stuck - stopping
```

Your spec now shows:

```toml
history = [
    { commit_created = "a1b2c3d" },
    { stuck = "OAuthProvider depends on SessionManager..." },
]
```

### To Continue

1. **Understand the problem**: Read the stuck message
2. **Fix it**: Edit hints, reorder commits, or manually adjust code
3. **Add a resolved entry**: Tell pravda what you changed

```toml
history = [
    { commit_created = "a1b2c3d" },
    { stuck = "OAuthProvider depends on SessionManager..." },
    { resolved = "Reordered commits 2 and 3 - SessionManager needs to come first" },
]
```

4. **Resume**:

```bash
pravda execute my-spec.toml
```

Pravda will retry with your resolution note as context.

### If You Don't Add Resolved

```bash
$ pravda execute my-spec.toml

Resuming from commit 2/3: feat: add OAuth provider support
  ✗ Previously stuck - add a `resolved` entry to continue
    Edit the spec file and add after the `stuck` entry:
    { resolved = "description of what you changed" }
```

This ensures the LLM knows what changed before retrying.

## Step 4: Review the Result

When complete:

```bash
# Check out the clean branch
git checkout my-feature-branch-clean

# Review the history
git log --oneline

# Compare to original (should be identical content)
git diff my-feature-branch
# (should show nothing)
```

### Handling WIP Commits

If pravda created WIP commits during fixes, you can squash them:

```bash
git checkout my-feature-branch-clean
git rebase -i origin/main
# Mark WIP commits as "fixup" to fold them into their parent
```

Or keep them for transparency about the reconstruction process.

## Common Patterns

### Circular Dependencies

**Problem**: Commit A needs something from commit B, but B depends on A.

**Solution**: Combine them into one commit, or extract the shared piece into a third commit that comes first.

```toml
# Before (broken)
[[commit]]
message = "feat: add TokenStore"
# ...

[[commit]]
message = "feat: add SessionManager"
# SessionManager uses TokenStore, but TokenStore uses SessionManager!

# After (fixed)
[[commit]]
message = "feat: add TokenStore and SessionManager"
hints = "These have circular dependencies - must be in same commit"
```

### Interleaved Changes

**Problem**: One file has changes for multiple logical commits.

**Solution**: Use hints to specify which changes belong where.

```toml
[[commit]]
message = "refactor: rename foo to bar"
hints = """
In src/lib.rs: ONLY the foo→bar renames (lines 10-50 of the diff).
Do NOT include the new baz() function - that's commit 2.
"""

[[commit]]
message = "feat: add baz functionality"
hints = """
In src/lib.rs: the new baz() function.
Builds on the bar rename from commit 1.
"""
```

### Missing Dependencies

**Problem**: Build fails because something from a later commit is needed.

**Solution**: Either reorder commits, or pull the dependency into the current commit's hints.

```toml
[[commit]]
message = "feat: add OAuth flow"
hints = """
Include the OAuthConfig struct even though it's "supposed" to be
in the config commit - we need it here for the code to compile.
"""
```

## Troubleshooting

### "Failed to find merge-base"

Your source and remote branches don't share history. Make sure:
- `source` is your feature branch name
- `remote` is the target branch (e.g., `origin/main`)
- Both branches exist

### "No more changes to extract"

The diff between cleaned and source is empty. This means either:
- All changes have been extracted (success!)
- The branches are identical (nothing to do)
- Wrong branch names in the spec

### Build Keeps Failing

If the LLM keeps getting stuck:
1. Check if changes are correctly split - maybe they need to be combined
2. Add more specific hints about what to include
3. Manually make the fix and add a `resolved` entry

### Want to Start Over

```bash
# Delete the clean branch
git branch -D my-feature-branch-clean

# Remove history from spec (or delete and recreate)
# Edit my-spec.toml, remove all `history = [...]` fields

# Run again
pravda execute my-spec.toml
```
