//! Scrollback-based terminal output for retcon execute.
//!
//! Prints agent activity and status messages to stdout with ANSI colors.
//! A single status line at the bottom shows the current commit progress,
//! redrawn in place using carriage return.

use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use crossterm::ExecutableCommand;
use crossterm::style::{Attribute, Color, ResetColor, SetAttribute, SetForegroundColor};
use crossterm::terminal::{Clear, ClearType};
use sacp::schema::{ContentBlock, SessionNotification, SessionUpdate};

use crate::execute::{CommitStatus, ExecuteHooks};

// =============================================================================
// Shared State
// =============================================================================

/// Shared state for coordinating stdout writes with the status line.
struct StatusState {
    /// Commit plan entries.
    plan: Vec<PlanEntry>,
    /// Whether the status line is currently drawn (needs clearing before normal output).
    status_drawn: bool,
}

#[derive(Clone)]
struct PlanEntry {
    message: String,
    status: CommitStatus,
}

impl StatusState {
    fn new() -> Self {
        Self {
            plan: Vec::new(),
            status_drawn: false,
        }
    }

    /// Clear the status line if it's drawn, so normal output can print cleanly.
    fn clear_status_line(&mut self) {
        if self.status_drawn {
            let mut stdout = io::stdout();
            let _ = stdout.execute(crossterm::cursor::MoveToColumn(0));
            let _ = stdout.execute(Clear(ClearType::CurrentLine));
            self.status_drawn = false;
        }
    }

    /// Draw/redraw the status line based on current plan state.
    fn draw_status_line(&mut self) {
        if self.plan.is_empty() {
            return;
        }

        let total = self.plan.len();
        let mut stdout = io::stdout();

        // Find the current item to display
        let (label, style) = if let Some((idx, entry)) = self
            .plan
            .iter()
            .enumerate()
            .find(|(_, e)| e.status == CommitStatus::InProgress)
        {
            (
                format!("[{}/{}] {}", idx + 1, total, entry.message),
                (Color::Yellow, true),
            )
        } else if let Some((idx, entry)) = self
            .plan
            .iter()
            .enumerate()
            .find(|(_, e)| e.status == CommitStatus::Stuck)
        {
            (
                format!("[{}/{}] STUCK: {}", idx + 1, total, entry.message),
                (Color::Red, true),
            )
        } else if self
            .plan
            .iter()
            .all(|e| e.status == CommitStatus::Completed)
        {
            (
                format!("[{}/{}] All commits complete", total, total),
                (Color::Green, false),
            )
        } else {
            // Find first pending
            let idx = self
                .plan
                .iter()
                .position(|e| e.status == CommitStatus::Pending)
                .unwrap_or(0);
            (
                format!("[{}/{}] {}", idx + 1, total, self.plan[idx].message),
                (Color::White, false),
            )
        };

        let _ = stdout.execute(crossterm::cursor::MoveToColumn(0));
        let _ = stdout.execute(Clear(ClearType::CurrentLine));
        let _ = stdout.execute(SetForegroundColor(style.0));
        if style.1 {
            let _ = stdout.execute(SetAttribute(Attribute::Bold));
        }
        let _ = write!(stdout, "{label}");
        if style.1 {
            let _ = stdout.execute(SetAttribute(Attribute::Reset));
        }
        let _ = stdout.execute(ResetColor);
        let _ = stdout.flush();

        self.status_drawn = true;
    }

    /// Print a line to stdout, managing the status line.
    fn println(&mut self, line: &str) {
        self.clear_status_line();
        println!("{line}");
        self.draw_status_line();
    }

    /// Print styled text to stdout, managing the status line.
    fn println_styled(&mut self, line: &str, color: Color, bold: bool) {
        self.clear_status_line();
        let mut stdout = io::stdout();
        let _ = stdout.execute(SetForegroundColor(color));
        if bold {
            let _ = stdout.execute(SetAttribute(Attribute::Bold));
        }
        println!("{line}");
        if bold {
            let _ = stdout.execute(SetAttribute(Attribute::Reset));
        }
        let _ = stdout.execute(ResetColor);
        self.draw_status_line();
    }

    /// Print agent text chunk — appends inline without a newline unless the chunk contains one.
    fn print_text_chunk(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }

        // If the text ends with a newline, the cursor will be on a fresh line,
        // so we can redraw the status. If it doesn't, leave the status cleared
        // since we're mid-line.
        self.clear_status_line();
        let mut stdout = io::stdout();
        let _ = write!(stdout, "{text}");
        let _ = stdout.flush();

        if text.ends_with('\n') {
            self.draw_status_line();
        }
    }
}

// =============================================================================
// TerminalObserver — implements ThinkObserver
// =============================================================================

/// Observer that prints agent activity to stdout with ANSI colors.
pub struct TerminalObserver {
    state: Arc<Mutex<StatusState>>,
}

impl determinishtic::ThinkObserver for TerminalObserver {
    fn on_prompt(&self, prompt: &str) {
        let mut state = self.state.lock().unwrap();
        state.println_styled("--- Prompt ---", Color::DarkGrey, false);
        for line in prompt.lines() {
            state.println_styled(line, Color::DarkGrey, false);
        }
        state.println_styled("--- End Prompt ---", Color::DarkGrey, false);
    }

    fn on_notification(&self, notification: &SessionNotification) {
        let mut state = self.state.lock().unwrap();
        match &notification.update {
            SessionUpdate::AgentMessageChunk(chunk) => {
                if let ContentBlock::Text(text) = &chunk.content {
                    state.print_text_chunk(&text.text);
                }
            }
            SessionUpdate::ToolCall(tool_call) => {
                state.println_styled(&format!("[tool] {}", tool_call.title), Color::Cyan, false);
            }
            SessionUpdate::ToolCallUpdate(update) => {
                if let Some(title) = &update.fields.title {
                    state.println_styled(&format!("[tool update] {title}"), Color::Cyan, false);
                }
            }
            _ => {}
        }
    }

    fn on_permission_request(&self, request: &sacp::schema::RequestPermissionRequest) {
        let mut state = self.state.lock().unwrap();
        let title = request
            .tool_call
            .fields
            .title
            .as_deref()
            .unwrap_or("unknown tool");
        state.println_styled(&format!("[permission] {title}"), Color::Yellow, false);
    }

    fn on_stop(&self, reason: &sacp::schema::StopReason) {
        let mut state = self.state.lock().unwrap();
        state.println_styled(&format!("Session stopped: {reason:?}"), Color::Green, false);
    }
}

// =============================================================================
// TerminalHooks — implements ExecuteHooks
// =============================================================================

/// Hooks that print status messages and manage the status line.
pub struct TerminalHooks {
    state: Arc<Mutex<StatusState>>,
}

impl ExecuteHooks for TerminalHooks {
    fn report(&self, message: &str) {
        let mut state = self.state.lock().unwrap();
        state.println(message);
    }

    fn plan_init(&self, commits: &[&str]) {
        let mut state = self.state.lock().unwrap();
        state.plan = commits
            .iter()
            .map(|msg| PlanEntry {
                message: msg.to_string(),
                status: CommitStatus::Pending,
            })
            .collect();
        state.draw_status_line();
    }

    fn plan_update(&self, commit_idx: usize, status: CommitStatus) {
        let mut state = self.state.lock().unwrap();
        if let Some(entry) = state.plan.get_mut(commit_idx) {
            entry.status = status;
        }
        state.clear_status_line();
        state.draw_status_line();
    }

    fn on_stuck(&self, reason: &str) -> Option<String> {
        let mut state = self.state.lock().unwrap();
        state.clear_status_line();
        state.println_styled(&format!("  ✗ Stuck: {reason}"), Color::Red, true);
        drop(state);

        println!();
        println!("Type your response to resume, SKIP to skip this commit, or Esc/Ctrl-C to stop:");
        print!("> ");
        let _ = io::stdout().flush();

        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(0) | Err(_) => None,
            Ok(_) => {
                let trimmed = input.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            }
        }
    }
}

// =============================================================================
// Constructor
// =============================================================================

/// Create a new terminal observer and hooks pair.
///
/// Returns `(observer, hooks)` — pass the observer to `execute_with_hooks`
/// as `Some(Arc::new(observer))`, and pass `&hooks` as the hooks argument.
pub fn new() -> (TerminalObserver, TerminalHooks) {
    let state = Arc::new(Mutex::new(StatusState::new()));
    let observer = TerminalObserver {
        state: state.clone(),
    };
    let hooks = TerminalHooks { state };
    (observer, hooks)
}
