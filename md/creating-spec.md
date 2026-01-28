# Creating a History Spec

This guide helps you (or an LLM assistant) create a history specification from a messy branch.

## Step 1: Understand the Changes

First, examine what actually changed:

```bash
# See the full diff against the target branch
git diff origin/main...feature-branch

# See the commit history (for context, not structure)
git log origin/main..feature-branch --oneline

# See which files changed
git diff origin/main...feature-branch --stat
```

The commit history shows how the work *happened*, but the diff shows what *actually changed*. Focus on the diff - that's what needs to be reconstructed.

## Step 2: Identify the Strands

Look for distinct "strands" of work in the diff:

- **Refactorings**: Code movement, renames, reorganization (no behavior change)
- **New features**: New functionality being added
- **Bug fixes**: Corrections to existing behavior
- **Cleanups**: Style changes, removing dead code, updating dependencies
- **Tests**: New or modified tests

Note which files belong to which strand. Watch for files that have changes from multiple strands - these need careful handling in the hints.

## Step 3: Design the Commit Sequence

Order commits to tell a clear story:

### Principles

1. **Refactorings before features**: Extract, rename, reorganize first. Then build on the clean foundation.

2. **One concept per commit**: Each commit should have a single purpose that can be stated clearly.

3. **Buildable is nice, not required**: Ideally each commit compiles and tests pass. But conceptual clarity trumps buildability - retcon will help fix compilation issues.

4. **Dependencies flow forward**: If commit B depends on commit A, A comes first.

### Common Patterns

**The Extract-Then-Use Pattern**
```
1. Extract X into its own module (refactor)
2. Add new capability to X (feature)
3. Use new capability in Y (feature)
```

**The Foundation-Then-Feature Pattern**
```
1. Add dependencies and configuration (setup)
2. Implement core functionality (feature)
3. Wire into existing system (integration)
4. Add tests (verification)
```

**The Parallel Concerns Pattern**
When changes are independent, order doesn't matter much:
```
1. Fix bug in auth (fix)
2. Update logging format (cleanup)
3. Add new API endpoint (feature)
```

## Step 4: Write the Spec

For each commit, write:

### The Message

Follow conventional commit style:
- `refactor:` - code changes that don't affect behavior
- `feat:` - new functionality
- `fix:` - bug fixes
- `test:` - adding or modifying tests
- `docs:` - documentation changes
- `chore:` - maintenance tasks

The message should be clear enough that someone reading `git log --oneline` understands the progression.

### The Hints

Hints guide the LLM in extracting the right changes. Include:

**What to include:**
```
hints = """
Files: src/auth.rs, src/validation.rs
Functions: validate_user(), validate_session()
The ValidationError enum and its From implementations.
"""
```

**What to exclude:**
```
hints = """
Extract ONLY the validation logic.
Do NOT include the OAuth changes to auth.rs (that's commit 3).
Do NOT include the new test file yet (that's commit 5).
"""
```

**Dependencies:**
```
hints = """
Builds on the validation module from commit 1.
Uses the OAuthConfig added in commit 2.
"""
```

**Tricky areas:**
```
hints = """
The token refresh has a race condition window - make sure
both the check and the refresh are included together.
"""
```

## Step 5: Validate the Spec

Before running retcon, sanity check:

1. **Coverage**: Do the commits cover all the changes in the diff?
2. **Order**: Do dependencies flow forward?
3. **Clarity**: Could someone understand the PR from reading just the commit messages?
4. **Hints**: Are the tricky parts called out?

## Example Analysis

Given this messy history:
```
a]1b2c3 WIP oauth stuff
d4e5f6 fix compile errors
g7h8i9 more oauth work
j0k1l2 extract validation (should have done this first)
m3n4o5 fix tests
p6q7r8 actually fix oauth token refresh
s9t0u1 remove debug logging
```

A good reconstruction might be:
```toml
[[commit]]
message = "refactor: extract validation logic to dedicated module"
hints = "The j0k1l2 changes, but applied first..."

[[commit]]
message = "feat: add OAuth provider authentication"
hints = "Core oauth from a1b2c3, g7h8i9, p6q7r8, minus debug logging..."

[[commit]]
message = "test: update tests for OAuth support"
hints = "Test fixes from m3n4o5..."
```

The messy "how it happened" becomes a clean "conceptual layers" story.
