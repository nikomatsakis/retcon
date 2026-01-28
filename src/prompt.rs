//! Generate LLM guidance for creating history specifications.

/// Generate the prompt that guides an LLM to create a history specification.
///
/// This is designed to be piped to an LLM along with context about the
/// repository and the changes to be organized.
#[must_use]
pub fn prompt() -> &'static str {
    include_str!("prompt.md")
}
