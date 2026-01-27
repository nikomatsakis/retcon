# Creating a History Specification

You are helping create a history specification for herodotus, a tool that reconstructs clean git history from messy branches.

## Your Task

Analyze the changes between the source branch and the remote branch, then create a TOML specification that describes logical commits to reconstruct.

## Output Format

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

## Guidelines for Splitting Commits

1. **One logical change per commit** - A commit should do one thing well
2. **Refactors before features** - Extract/reorganize code in separate commits before adding new functionality
3. **Tests with implementation** - Include tests in the same commit as the code they test, unless it's a pure test addition
4. **Order matters** - Each commit should build successfully. Plan the order so dependencies come first.
5. **Keep hints specific** - Name files, functions, and modules. Vague hints are unhelpful.

## Commit Message Format

Use conventional commits style:
- `feat:` new feature
- `fix:` bug fix
- `refactor:` code reorganization without behavior change
- `test:` adding or updating tests
- `docs:` documentation changes
- `chore:` maintenance tasks

## Example Analysis

If you see changes that:
- Move code to a new module
- Add a new feature using that module
- Add tests for the feature

Split into:
1. `refactor: extract X into dedicated module` (just the move)
2. `feat: add Y functionality` (the new feature)
3. `test: add tests for Y` (if tests are substantial)

Or if tests are small, include them with the feature.

## What to Examine

To create a good specification:
1. Look at the diff between source and remote
2. Identify logical groupings of changes
3. Determine the order that allows each commit to build
4. Write specific hints for each commit
