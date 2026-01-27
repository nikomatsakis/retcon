//! Herodotus: Reconstruct clean git history from messy branches using LLM assistance.
//!
//! Herodotus takes a history specification (TOML) describing logical commits and
//! reconstructs them from a messy source branch, creating a clean history suitable
//! for code review.
//!
//! # Architecture
//!
//! - **Spec**: Parse and manipulate history specifications
//! - **Execute**: Run the reconstruction loop with LLM assistance
//! - **Prompt**: Generate guidance for creating specifications

mod execute;
mod prompt;
mod spec;

pub use execute::execute;
pub use prompt::prompt;
pub use spec::{CommitSpec, HistoryEntry, HistorySpec};
