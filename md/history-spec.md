# History Specification

The history specification is a TOML file that describes:

- Which branches to work with
- The logical commits to create
- Hints to guide the LLM in extracting each commit
- Execution history tracking progress and state

## Format

```toml
# Branches
source = "feature-oauth"           # Branch containing your changes
remote = "origin/main"             # Target branch for the PR
cleaned = "feature-oauth-clean"    # Branch to create with clean history

# Commits (in order)
[[commit]]
message = "refactor: extract user validation into dedicated module"
hints = """
Move validate_user, validate_session, and related helpers from lib.rs to validation.rs.
Pure reorganization - no behavior changes.
Tests will need import updates.
"""
history = [
    { commit_created = "a1b2c3d" },
    "complete",
]

[[commit]]
message = "feat: add OAuth 2.0 provider authentication"
hints = """
New oauth.rs module with OAuthProvider trait and implementations.
Token refresh logic is subtle - ensure the refresh_token flow is complete.
Builds on validation module from previous commit.
"""
history = [
    { commit_created = "e4f5g6h" },
    { commit_created = "f7g8h9i" },  # WIP fix
    "complete",
]

[[commit]]
message = "feat: add backward compatibility shim for legacy auth"
hints = """
LegacyAuthAdapter in compat.rs wraps old auth calls.
Should be thin - mostly delegates to new OAuth internals.
"""
# history absent - not yet started
```

## Fields

### Branch Configuration

| Field | Required | Description |
|-------|----------|-------------|
| `source` | Yes | The branch containing all your changes (the messy history) |
| `remote` | Yes | The upstream branch this will merge into (e.g., `origin/main`) |
| `cleaned` | Yes | The new branch to create with reconstructed history |

The cleaned branch starts from `git merge-base source remote` - the point where your work diverged from the target.

### Commit Entries

Each `[[commit]]` represents one logical commit in the final history, applied in order.

| Field | Required | Description |
|-------|----------|-------------|
| `message` | Yes | The main commit message (first line) |
| `hints` | No | Guidance for the LLM on what changes belong in this commit |
| `history` | No | Execution log tracking commits created and status (managed by herodotus) |

### History Entries

The `history` field is a vector that herodotus appends to as it works. Each entry is one of:

```rust
enum HistoryEntry {
    CommitCreated { hash: String },  // A commit was created (main or WIP fix)
    Stuck { summary: String },       // LLM assessed it cannot proceed
    Complete,                        // This logical commit is done
}
```

In TOML:
```toml
history = [
    { commit_created = "a1b2c3d" },
    { commit_created = "b4c5d6e" },  # WIP fix
    { stuck = "Missing type definition - may need to reorder commits" },
]
```

The history tells you the commit's status:

| History state | Meaning |
|---------------|---------|
| Absent or empty | Not yet started |
| Ends with `commit_created` | In progress, build may not pass yet |
| Ends with `stuck` | Paused, needs human intervention |
| Ends with `complete` | Done, proceed to next commit |

When resuming, herodotus finds the first commit whose history doesn't end in `complete` and continues from there.

### Writing Good Hints

Hints help the LLM extract the right changes. Good hints:

- **Name specific files or functions** that should be modified
- **Describe the nature of changes** (refactor, new feature, fix, etc.)
- **Note dependencies** on previous commits
- **Flag tricky areas** where care is needed
- **Mention what to exclude** if changes from multiple concerns touch the same files

Bad hints are vague ("make it work") or redundant with the message.

## Example: OAuth Feature

```toml
source = "feature-oauth"
remote = "origin/main"
cleaned = "feature-oauth-clean"

[[commit]]
message = "refactor: extract validation logic to prepare for OAuth"
hints = """
Create src/validation.rs with:
- validate_credentials() moved from auth.rs
- validate_session() moved from session.rs
- ValidationError enum

Update imports in auth.rs, session.rs, and tests.
No behavior changes - just reorganization.
"""

[[commit]]
message = "feat: add OAuth configuration and dependencies"
hints = """
- Add oauth2 crate to Cargo.toml
- Create src/oauth/mod.rs with OAuthConfig struct
- Add oauth section to config.toml parsing

Does NOT include the actual OAuth flow yet - just setup.
"""

[[commit]]
message = "feat: implement OAuth provider authentication flow"
hints = """
Main implementation in src/oauth/:
- provider.rs: OAuthProvider trait
- google.rs, github.rs: provider implementations
- token.rs: token refresh logic (careful with the refresh window)

Wire into existing auth system via AuthMethod enum variant.
"""

[[commit]]
message = "feat: add backward compatibility for password auth"
hints = """
LegacyPasswordAuth adapter in src/compat.rs.
Thin wrapper - delegates to new validation module.
Add deprecation warnings when legacy path is used.
"""

[[commit]]
message = "test: add OAuth integration tests"
hints = """
New test file tests/oauth_integration.rs.
Mock provider for testing without real OAuth endpoints.
Test token refresh, expiry handling, error cases.
"""
```
