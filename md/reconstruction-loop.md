# The Reconstruction Loop

Herodotus reconstructs clean history through a deterministic loop with LLM-powered steps.

## Algorithm

```
1. Load history spec from TOML
2. Create cleaned branch from merge-base(source, remote) if needed
3. Find first commit where history doesn't end in "complete"
4. For each remaining commit:
   a. Compute diff: cleaned..source
   b. LLM: Extract relevant changes for this commit from diff
   c. LLM: Apply changes and create commit
   d. Append CommitCreated { hash } to history, save TOML
   e. Loop:
      - Run build and tests
      - If pass: append Complete to history, save TOML, next commit
      - If fail:
        - Compute diff: cleaned..source
        - LLM: Assess - can you make progress?
          - If yes: fix, create WIP commit, append CommitCreated, save TOML
          - If no: append Stuck { summary }, save TOML, stop
5. Report results
```

## Deterministic vs LLM Boundaries

Following the patchwork philosophy ("do things deterministically that are deterministic"):

| Deterministic (Rust) | LLM-powered |
|---------------------|-------------|
| Parse TOML spec | Extract relevant diff hunks |
| Create/checkout branches | Decide what changes belong together |
| Compute diffs | Apply changes to files |
| Run build/test commands | Write commit messages |
| Track loop state | Diagnose build failures |
| | Determine what else to pull from source |

## The Fix Loop

When a commit doesn't build or tests fail, herodotus enters a fix loop:

```
┌─────────────────────────────────────────────────────────┐
│                     Fix Loop                            │
│                                                         │
│  ┌──────────┐    ┌───────────┐    ┌──────────────────┐ │
│  │ Run      │───▶│ Pass?     │─Y─▶│ Append Complete  │ │
│  │ build    │    └───────────┘    │ Next commit      │ │
│  └──────────┘          │N         └──────────────────┘ │
│       ▲                ▼                               │
│       │         ┌───────────┐                          │
│       │         │ Compute   │                          │
│       │         │ new diff  │                          │
│       │         └───────────┘                          │
│       │                │                               │
│       │                ▼                               │
│       │         ┌─────────────────┐                    │
│       │         │ LLM: Can you    │                    │
│       │         │ make progress?  │                    │
│       │         └─────────────────┘                    │
│       │           │Y          │N                       │
│       │           ▼           ▼                        │
│       │    ┌──────────┐  ┌──────────────┐              │
│       │    │ Fix +    │  │ Append Stuck │              │
│       │    │ WIP      │  │ Stop         │              │
│       │    │ commit   │  └──────────────┘              │
│       │    └──────────┘                                │
│       │           │                                    │
│       │           ▼                                    │
│       │    ┌──────────────────┐                        │
│       │    │ Append           │                        │
│       │    │ CommitCreated    │                        │
│       │    │ Save TOML        │                        │
│       │    └──────────────────┘                        │
│       │           │                                    │
│       └───────────┘                                    │
└─────────────────────────────────────────────────────────┘
```

Key insight: each iteration recomputes the diff from cleaned to source. This means the LLM can pull in additional changes that it now realizes are needed - perhaps a helper function, a type definition, or an import that the original extraction missed.

### LLM Progress Assessment

Instead of a fixed iteration limit, the LLM assesses after each failed build:

> "Given the build errors and the remaining diff from source, can you make progress? Or are you stuck?"

The LLM returns one of:
- **Progress**: "I can fix this" → creates WIP commit, loop continues
- **Stuck**: "I need help: <summary>" → appends `Stuck`, stops

This lets the LLM recognize situations it can't resolve:
- Circular dependencies between commits
- Missing context it can't infer
- Ambiguous hints that need clarification
- Changes that don't belong together

### WIP Commits

Fix iterations create commits prefixed with `WIP:`:

```
feat: add OAuth provider authentication
WIP: add missing TokenRefresh import
WIP: include refresh_token helper that was needed
```

This creates an honest record of what happened. The history tracks every commit:

```toml
history = [
    { commit_created = "a1b2c3d" },  # initial attempt
    { commit_created = "e4f5g6h" },  # WIP: add missing import
    { commit_created = "i7j8k9l" },  # WIP: include helper
    "complete",
]
```

Options for handling WIP commits:
- **Keep them**: Transparent history of the reconstruction
- **Squash manually**: `git rebase -i` to fold WIPs into their parent
- **Future**: `--squash-wip` flag to auto-collapse

### Resuming After Stuck

When herodotus encounters a `Stuck` entry, it:

1. Shows the summary to the user
2. Allows intervention (edit hints, reorder commits, manual fixes)
3. On next run, sees history ends in `Stuck`, asks LLM to try again with fresh context

The TOML file is the complete state - you can edit it, inspect the history, and resume at any point.

## Tools Provided to LLM

During reconstruction, the LLM has access to:

| Tool | Purpose |
|------|---------|
| `read_file` | Read file contents from working tree |
| `write_file` | Write file contents |
| `read_diff` | Get the current cleaned..source diff |
| `run_build` | Execute build command, get output |
| `run_tests` | Execute test command, get output |
| `create_commit` | Stage all changes and commit with message |

The LLM does NOT have:
- Direct git access (herodotus manages branches)
- Network access
- Ability to modify the source branch

## Example Session

```
$ herodotus reconstruct history-spec.toml

Loading spec: history-spec.toml
  Source: feature-oauth
  Remote: origin/main
  Cleaned: feature-oauth-clean

Creating cleaned branch from merge-base...
  Base commit: a1b2c3d

Commit 1/4: refactor: extract validation logic
  Computing diff (847 lines)...
  Extracting relevant changes...
  Creating commit...
  Building... PASS
  Tests... PASS
  ✓ Commit created

Commit 2/4: feat: add OAuth configuration
  Computing diff (623 lines)...
  Extracting relevant changes...
  Creating commit...
  Building... FAIL (missing import)
  Fix attempt 1:
    Consulting diff for missing pieces...
    Adding TokenConfig import...
    Creating WIP commit...
  Building... PASS
  Tests... PASS
  ✓ Commit created (1 WIP)

Commit 3/4: feat: implement OAuth flow
  Computing diff (412 lines)...
  ...

Complete!
  4 logical commits created
  2 WIP fix commits
  Branch: feature-oauth-clean

To squash WIP commits:
  git checkout feature-oauth-clean
  git rebase -i HEAD~6
```

### Example: Stuck and Resume

```
$ herodotus reconstruct history-spec.toml

...
Commit 3/4: feat: implement OAuth flow
  Computing diff (412 lines)...
  Extracting relevant changes...
  Creating commit...
  Building... FAIL (undefined type TokenStore)

  Assessing progress...
  Fix attempt 1:
    Adding TokenStore from diff...
    Creating WIP commit...
  Building... FAIL (TokenStore needs SessionManager)

  Assessing progress...
  LLM: "I'm stuck. TokenStore depends on SessionManager which depends on
        TokenStore - circular dependency. The commits may need reordering,
        or these changes need to be in the same commit."

  Appending Stuck to history...
  Saving history-spec.toml...

Stopped at commit 3/4. See history-spec.toml for details.

$ # User edits hints or reorders commits...
$ vim history-spec.toml

$ herodotus reconstruct history-spec.toml

Resuming from commit 3/4 (previously stuck)...
```
