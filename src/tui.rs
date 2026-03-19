//! Two-pane TUI for retcon execute.
//!
//! Top pane: live agent activity (text, tool calls, permissions).
//! Bottom pane: commit plan progress.

use std::io;
use std::sync::{Arc, Mutex};

use crossterm::ExecutableCommand;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use sacp::schema::{ContentBlock, SessionNotification, SessionUpdate};

use crate::execute::{CommitStatus, ExecuteHooks};

// =============================================================================
// Shared State
// =============================================================================

/// Shared state between the async observer/hooks and the sync TUI render loop.
#[derive(Debug, Default)]
struct TuiState {
    /// Lines of agent activity for the snooping pane.
    agent_lines: Vec<AgentLine>,

    /// Commit plan entries.
    plan: Vec<PlanEntry>,
    /// Whether execution has finished.
    done: bool,
    /// Final error message, if any.
    error: Option<String>,
}

#[derive(Debug, Clone)]
struct AgentLine {
    kind: LineKind,
    text: String,
}

#[derive(Debug, Clone, Copy)]
enum LineKind {
    Text,
    Tool,
    Permission,
    Status,
}

#[derive(Debug, Clone)]
struct PlanEntry {
    message: String,
    status: CommitStatus,
}

// =============================================================================
// TuiObserver — implements ThinkObserver
// =============================================================================

/// Observer that extracts displayable content from session messages.
pub struct TuiObserver {
    state: Arc<Mutex<TuiState>>,
}

impl determinishtic::ThinkObserver for TuiObserver {
    fn on_prompt(&self, prompt: &str) {
        let mut state = self.state.lock().unwrap();
        state.agent_lines.push(AgentLine {
            kind: LineKind::Status,
            text: "--- Prompt ---".to_string(),
        });
        for line in prompt.lines() {
            state.agent_lines.push(AgentLine {
                kind: LineKind::Status,
                text: line.to_string(),
            });
        }
        state.agent_lines.push(AgentLine {
            kind: LineKind::Status,
            text: "--- End Prompt ---".to_string(),
        });
    }

    fn on_notification(&self, notification: &SessionNotification) {
        let mut state = self.state.lock().unwrap();
        match &notification.update {
            SessionUpdate::AgentMessageChunk(chunk) => {
                if let ContentBlock::Text(text) = &chunk.content {
                    // Agent text arrives in small token-sized chunks.
                    // Append to the last Text line; start a new line on \n.
                    for (i, segment) in text.text.split('\n').enumerate() {
                        if i > 0 {
                            // Newline boundary — start a new text line
                            state.agent_lines.push(AgentLine {
                                kind: LineKind::Text,
                                text: String::new(),
                            });
                        }
                        if !segment.is_empty() {
                            // Append to the current text line, or start one
                            if let Some(last) = state
                                .agent_lines
                                .last_mut()
                                .filter(|l| matches!(l.kind, LineKind::Text))
                            {
                                last.text.push_str(segment);
                            } else {
                                state.agent_lines.push(AgentLine {
                                    kind: LineKind::Text,
                                    text: segment.to_string(),
                                });
                            }
                        }
                    }
                }
            }
            SessionUpdate::ToolCall(tool_call) => {
                state.agent_lines.push(AgentLine {
                    kind: LineKind::Tool,
                    text: format!("[tool] {}", tool_call.title),
                });
            }
            SessionUpdate::ToolCallUpdate(update) => {
                if let Some(title) = &update.fields.title {
                    state.agent_lines.push(AgentLine {
                        kind: LineKind::Tool,
                        text: format!("[tool update] {title}"),
                    });
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
        state.agent_lines.push(AgentLine {
            kind: LineKind::Permission,
            text: format!("[permission] {title}"),
        });
    }

    fn on_stop(&self, reason: &sacp::schema::StopReason) {
        let mut state = self.state.lock().unwrap();
        state.agent_lines.push(AgentLine {
            kind: LineKind::Status,
            text: format!("Session stopped: {reason:?}"),
        });
    }
}

// =============================================================================
// TuiHooks — implements ExecuteHooks
// =============================================================================

/// Hooks that feed structured plan events into the TUI state.
pub struct TuiHooks {
    state: Arc<Mutex<TuiState>>,
}

impl ExecuteHooks for TuiHooks {
    fn report(&self, message: &str) {
        let mut state = self.state.lock().unwrap();
        state.agent_lines.push(AgentLine {
            kind: LineKind::Status,
            text: message.to_string(),
        });
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
    }

    fn plan_update(&self, commit_idx: usize, status: CommitStatus) {
        let mut state = self.state.lock().unwrap();
        if let Some(entry) = state.plan.get_mut(commit_idx) {
            entry.status = status;
        }
    }
}

// =============================================================================
// TuiApp — the ratatui event loop
// =============================================================================

/// The TUI application. Owns the terminal and runs the render/input loop.
pub struct TuiApp {
    state: Arc<Mutex<TuiState>>,
}

impl TuiApp {
    /// Create a new TUI app and return it along with the observer and hooks.
    pub fn new() -> (Self, TuiObserver, TuiHooks) {
        let state = Arc::new(Mutex::new(TuiState::default()));
        let app = Self {
            state: state.clone(),
        };
        let observer = TuiObserver {
            state: state.clone(),
        };
        let hooks = TuiHooks {
            state: state.clone(),
        };
        (app, observer, hooks)
    }

    /// Signal that execution is done (success or failure).
    pub fn signal_done(&self, result: Result<(), String>) {
        let mut state = self.state.lock().unwrap();
        state.done = true;
        if let Err(e) = result {
            state.error = Some(e);
        }
    }

    /// Run the TUI event loop. Blocks the current thread.
    ///
    /// Returns when the user presses 'q' or execution finishes.
    pub fn run(&self) -> io::Result<()> {
        // Set up panic hook to restore terminal on panic
        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = terminal::disable_raw_mode();
            let _ = io::stdout().execute(LeaveAlternateScreen);
            original_hook(info);
        }));

        // Set up terminal
        terminal::enable_raw_mode()?;
        io::stdout().execute(EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(io::stdout());
        let mut terminal = Terminal::new(backend)?;

        let result = self.event_loop(&mut terminal);

        // Restore terminal
        terminal::disable_raw_mode()?;
        io::stdout().execute(LeaveAlternateScreen)?;

        result
    }

    fn event_loop(&self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
        loop {
            // Draw
            terminal.draw(|frame| self.draw(frame))?;

            // Poll for input with a short timeout so we keep redrawing
            if event::poll(std::time::Duration::from_millis(50))? {
                if let Event::Key(key) = event::read()? {
                    if self.handle_key(key) {
                        return Ok(());
                    }
                }
            }

            // Check if done and user should see final state
            let state = self.state.lock().unwrap();
            if state.done {
                drop(state);
                // Draw one more time to show final state, then wait for 'q'
                terminal.draw(|frame| self.draw(frame))?;
                loop {
                    if let Event::Key(key) = event::read()? {
                        if matches!(key.code, KeyCode::Char('q') | KeyCode::Char('Q'))
                            || (key.code == KeyCode::Char('c')
                                && key.modifiers.contains(KeyModifiers::CONTROL))
                        {
                            return Ok(());
                        }
                    }
                }
            }
        }
    }

    fn handle_key(&self, key: KeyEvent) -> bool {
        matches!(key.code, KeyCode::Char('q') | KeyCode::Char('Q'))
            || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
    }

    fn draw(&self, frame: &mut Frame) {
        let state = self.state.lock().unwrap();
        let area = frame.area();

        // Vertical split: agent 2/3 top, progress 1/3 bottom
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(67), Constraint::Percentage(33)])
            .split(area);

        // Agent pane
        self.draw_agent_pane(frame, chunks[0], &state);

        // Progress pane
        self.draw_progress_pane(frame, chunks[1], &state);
    }

    fn draw_agent_pane(&self, frame: &mut Frame, area: Rect, state: &TuiState) {
        let title = if state.done {
            if let Some(ref err) = state.error {
                format!(" Agent (DONE - error: {err}) ")
            } else {
                " Agent (DONE) ".to_string()
            }
        } else {
            " Agent ".to_string()
        };

        let block = Block::default().title(title).borders(Borders::ALL);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Show the tail of agent lines, auto-scrolling to bottom.
        let lines: Vec<Line> = state
            .agent_lines
            .iter()
            .map(|line| {
                let style = match line.kind {
                    LineKind::Text => Style::default(),
                    LineKind::Tool => Style::default().fg(Color::Cyan),
                    LineKind::Permission => Style::default().fg(Color::Yellow),
                    LineKind::Status => Style::default().fg(Color::Green).bold(),
                };
                Line::styled(&line.text, style)
            })
            .collect();

        // Compute wrapped line count to auto-scroll to the bottom.
        let width = inner.width as usize;
        let wrapped_height: usize = if width == 0 {
            lines.len()
        } else {
            lines
                .iter()
                .map(|line| {
                    let len = line.width();
                    if len == 0 {
                        1
                    } else {
                        (len + width - 1) / width
                    }
                })
                .sum()
        };
        let scroll = (wrapped_height as u16).saturating_sub(inner.height);
        let paragraph = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0));
        frame.render_widget(paragraph, inner);
    }

    fn draw_progress_pane(&self, frame: &mut Frame, area: Rect, state: &TuiState) {
        let block = Block::default().title(" Progress ").borders(Borders::ALL);
        let visible_height = block.inner(area).height as usize;

        // Find the "focus" item: first InProgress, else first non-Completed.
        // If all complete (or empty), focus the last item so the list is bottom-justified.
        let focus_idx = state
            .plan
            .iter()
            .position(|e| e.status == CommitStatus::InProgress)
            .or_else(|| {
                state
                    .plan
                    .iter()
                    .position(|e| e.status != CommitStatus::Completed)
            });

        // max_skip ensures we never leave blank lines at the bottom.
        let max_skip = state.plan.len().saturating_sub(visible_height);
        let skip = match focus_idx {
            Some(idx) => {
                // Center the focus item, clamped to avoid blank lines
                let half = visible_height / 2;
                idx.saturating_sub(half).min(max_skip)
            }
            None => {
                // All complete or empty — bottom-justify (show the tail)
                max_skip
            }
        };

        let items: Vec<ListItem> = state
            .plan
            .iter()
            .enumerate()
            .skip(skip)
            .take(visible_height)
            .map(|(i, entry)| {
                let (icon, style) = match entry.status {
                    CommitStatus::Pending => (" ", Style::default().dim()),
                    CommitStatus::InProgress => ("~", Style::default().fg(Color::Yellow).bold()),
                    CommitStatus::Completed => ("x", Style::default().fg(Color::Green)),
                    CommitStatus::Stuck => ("!", Style::default().fg(Color::Red).bold()),
                };
                ListItem::new(Line::styled(
                    format!("[{icon}] {}. {}", i + 1, entry.message),
                    style,
                ))
            })
            .collect();

        let list = List::new(items).block(block);
        frame.render_widget(list, area);
    }
}
