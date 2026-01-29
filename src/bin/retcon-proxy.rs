//! ACP proxy binary for retcon.
//!
//! This exposes retcon functionality as an MCP server that can be used
//! within an ACP agent chain. The key advantage is that the execute tool
//! reuses the existing agent connection for LLM work, rather than creating
//! its own connection.
//!
//! This proxy also provides a `/retcon:rewrite-git-history` slash command
//! that guides the user through creating a history specification.

use std::sync::RwLock;

use determinishtic::Determinishtic;
use sacp::mcp_server::{McpConnectionTo, McpServer};
use sacp::schema::{
    AvailableCommand, ContentBlock, ContentChunk, Plan, PlanEntry, PlanEntryPriority,
    PlanEntryStatus, PromptRequest, SessionId, SessionNotification, SessionUpdate, TextContent,
};
use sacp::{Agent, Client, Conductor, ConnectionTo, Proxy};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The slash command name for rewriting git history.
const REWRITE_SLASH_COMMAND: &str = "retcon:rewrite-git-history";

/// Parameters for the execute-git-rewrite tool.
#[derive(Debug, Deserialize, JsonSchema)]
struct ExecuteParams {
    /// The TOML specification content describing the commits to create
    toml_spec: String,
    /// Optional build command (default: "cargo check --all --workspace")
    build_command: Option<String>,
    /// Optional test command (default: "cargo test --all --workspace")
    test_command: Option<String>,
    /// Skip build verification
    skip_build: Option<bool>,
    /// Skip test verification
    skip_test: Option<bool>,
}

/// Status of the rewrite execution.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "status")]
enum ExecuteStatus {
    /// All commits were successfully reconstructed
    Complete,
    /// The LLM got stuck on a specific commit
    Stuck {
        /// 0-indexed commit number
        commit_index: usize,
        /// The commit message
        commit_message: String,
        /// Why the LLM got stuck
        reason: String,
    },
    /// An error occurred during execution
    Error {
        /// The error message
        message: String,
    },
}

/// Result of the execute-git-rewrite tool.
#[derive(Debug, Serialize, JsonSchema)]
struct ExecuteResult {
    /// The status of the execution
    status: ExecuteStatus,
    /// The updated TOML spec with execution history
    updated_toml: String,
}

// =============================================================================
// ACP Hooks Implementation
// =============================================================================

/// Hooks implementation that sends progress updates over ACP.
///
/// Sends both text messages (via AgentMessageChunk) and plan updates
/// (via Plan) to the client.
struct AcpHooks {
    connection: ConnectionTo<Conductor>,
    session_id: SessionId,
    /// Commit messages for building the plan. Stored on plan_init.
    commits: RwLock<Vec<String>>,
    /// Current status of each commit. Updated on plan_update.
    statuses: RwLock<Vec<PlanEntryStatus>>,
}

impl AcpHooks {
    fn new(connection: ConnectionTo<Conductor>, session_id: SessionId) -> Self {
        Self {
            connection,
            session_id,
            commits: RwLock::new(Vec::new()),
            statuses: RwLock::new(Vec::new()),
        }
    }

    /// Extract session ID from an acp:UUID URL.
    fn session_id_from_acp_url(acp_url: &str) -> SessionId {
        // Format is "acp:UUID" - strip the prefix
        let id = acp_url.strip_prefix("acp:").unwrap_or(acp_url);
        SessionId::new(id)
    }

    /// Send a plan update notification to the client.
    fn send_plan(&self) {
        let commits = self.commits.read().unwrap();
        let statuses = self.statuses.read().unwrap();

        let entries: Vec<PlanEntry> = commits
            .iter()
            .zip(statuses.iter())
            .map(|(message, status)| {
                PlanEntry::new(message.clone(), PlanEntryPriority::Medium, status.clone())
            })
            .collect();

        let plan = Plan::new(entries);
        let notification =
            SessionNotification::new(self.session_id.clone(), SessionUpdate::Plan(plan));

        // Ignore errors - we're in a sync context and can't do much about failures
        let _ = self.connection.send_notification_to(Client, notification);
    }

    /// Send a text message notification to the client.
    fn send_message(&self, text: &str) {
        let chunk = ContentChunk::new(ContentBlock::Text(TextContent::new(text)));
        let notification = SessionNotification::new(
            self.session_id.clone(),
            SessionUpdate::AgentMessageChunk(chunk),
        );

        let _ = self.connection.send_notification_to(Client, notification);
    }
}

impl retcon::ExecuteHooks for AcpHooks {
    fn report(&self, message: &str) {
        self.send_message(message);
    }

    fn plan_init(&self, commits: &[&str]) {
        {
            let mut stored = self.commits.write().unwrap();
            let mut statuses = self.statuses.write().unwrap();

            stored.clear();
            statuses.clear();

            for commit in commits {
                stored.push((*commit).to_string());
                statuses.push(PlanEntryStatus::Pending);
            }
        }
        self.send_plan();
    }

    fn plan_update(&self, commit_idx: usize, status: retcon::CommitStatus) {
        {
            let mut statuses = self.statuses.write().unwrap();
            if commit_idx < statuses.len() {
                // Map our CommitStatus to PlanEntryStatus
                // Note: PlanEntryStatus doesn't have "Stuck" - we keep it as InProgress
                // and rely on the text message to communicate the stuck state
                statuses[commit_idx] = match status {
                    retcon::CommitStatus::Pending => PlanEntryStatus::Pending,
                    retcon::CommitStatus::InProgress => PlanEntryStatus::InProgress,
                    retcon::CommitStatus::Completed => PlanEntryStatus::Completed,
                    retcon::CommitStatus::Stuck => PlanEntryStatus::InProgress, // Best we can do
                };
            }
        }
        self.send_plan();
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("retcon-proxy starting");

    let mcp_server = McpServer::builder("retcon")
        .instructions(
            "Git history rewriting tools. Use execute-git-rewrite to run a rewrite \
             from a TOML specification. The tool will use the agent to extract and \
             apply changes, creating clean commits.",
        )
        .tool_fn(
            "execute-git-rewrite",
            "Execute a git history rewrite from a TOML specification. \
             Returns the updated spec with execution history and a status \
             indicating completion or where it got stuck.",
            async |params: ExecuteParams, cx: McpConnectionTo<Conductor>| {
                execute_tool(params, cx).await
            },
            sacp::tool_fn!(),
        )
        .build();

    Proxy
        .builder()
        .with_mcp_server(mcp_server)
        // Intercept PromptRequest from client to detect slash command
        .on_receive_request_from(
            Client,
            async |req: PromptRequest, responder, cx| {
                if is_rewrite_command(&req) {
                    // Replace the prompt with our canned prompt
                    let modified = replace_with_canned_prompt(req);
                    cx.send_request_to(Agent, modified)
                        .forward_response_to(responder)
                } else {
                    // Forward unmodified
                    cx.send_request_to(Agent, req)
                        .forward_response_to(responder)
                }
            },
            sacp::on_receive_request!(),
        )
        // Intercept SessionNotification from agent to inject our command
        .on_receive_notification_from(
            Agent,
            async |mut notif: SessionNotification, cx| {
                // Check if this is an AvailableCommandsUpdate
                if let SessionUpdate::AvailableCommandsUpdate(ref mut update) = notif.update {
                    // Inject our command
                    update.available_commands.push(AvailableCommand::new(
                        REWRITE_SLASH_COMMAND,
                        "Create a clean git history from messy commits",
                    ));
                }
                // Forward the (possibly modified) notification to client
                cx.send_notification_to(Client, notif)
            },
            sacp::on_receive_notification!(),
        )
        .connect_to(sacp_tokio::Stdio::new())
        .await?;

    Ok(())
}

/// Check if the prompt request is the rewrite-git-history command.
fn is_rewrite_command(request: &PromptRequest) -> bool {
    // Extract text from the prompt
    let text: String = request
        .prompt
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(TextContent { text, .. }) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ");

    let text = text.trim();

    // Check for /retcon:rewrite-git-history (with or without the leading slash)
    text == format!("/{}", REWRITE_SLASH_COMMAND) || text == REWRITE_SLASH_COMMAND
}

/// Replace the prompt with the canned retcon prompt.
fn replace_with_canned_prompt(mut request: PromptRequest) -> PromptRequest {
    // Replace the prompt content with our canned prompt
    request.prompt = vec![ContentBlock::Text(TextContent::new(retcon::prompt()))];
    request
}

async fn execute_tool(
    params: ExecuteParams,
    cx: McpConnectionTo<Conductor>,
) -> Result<ExecuteResult, sacp::Error> {
    // 1. Parse the TOML spec
    let spec = retcon::HistorySpec::from_toml(&params.toml_spec)
        .map_err(|e| sacp::Error::invalid_params().data(e.to_string()))?;

    // 2. Find the git repository (from current directory)
    let git = retcon::Git::discover(std::path::Path::new("."))
        .map_err(|e| sacp::Error::internal_error().data(e.to_string()))?;

    // 3. Build config
    let config = retcon::ExecuteConfig {
        build_command: if params.skip_build.unwrap_or(false) {
            None
        } else {
            params
                .build_command
                .or_else(|| Some("cargo check --all --workspace".to_string()))
        },
        test_command: if params.skip_test.unwrap_or(false) {
            None
        } else {
            params
                .test_command
                .or_else(|| Some("cargo test --all --workspace".to_string()))
        },
    };

    // 4. Create Determinishtic and AcpHooks from the connection
    let connection = cx.connection_to();
    let session_id = AcpHooks::session_id_from_acp_url(&cx.acp_url());
    let hooks = AcpHooks::new(connection.clone(), session_id);
    let d = Determinishtic::from_connection(connection);

    // 5. Run execute with ACP hooks for progress feedback
    let result = retcon::execute_with_connection(&d, spec, &git, &config, &hooks).await;

    // 6. Determine status from result
    let (spec, error) = match result {
        Ok(spec) => (spec, None),
        Err((spec, e)) => (spec, Some(e)),
    };

    let updated_toml = spec
        .to_toml()
        .map_err(|e| sacp::Error::internal_error().data(e.to_string()))?;

    let status = if let Some(e) = error {
        ExecuteStatus::Error {
            message: e.to_string(),
        }
    } else if spec.commits.iter().all(|c| c.is_complete()) {
        ExecuteStatus::Complete
    } else if let Some((idx, commit)) = spec.commits.iter().enumerate().find(|(_, c)| c.is_stuck())
    {
        let reason = commit
            .history
            .last()
            .and_then(|h| match h {
                retcon::HistoryEntry::Stuck(r) => Some(r.clone()),
                _ => None,
            })
            .unwrap_or_else(|| "Unknown reason".to_string());
        ExecuteStatus::Stuck {
            commit_index: idx,
            commit_message: commit.message.clone(),
            reason,
        }
    } else {
        ExecuteStatus::Complete
    };

    Ok(ExecuteResult {
        status,
        updated_toml,
    })
}
