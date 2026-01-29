//! ACP proxy binary for retcon.
//!
//! This exposes retcon functionality as an MCP server that can be used
//! within an ACP agent chain. The key advantage is that the execute tool
//! reuses the existing agent connection for LLM work, rather than creating
//! its own connection.
//!
//! This proxy also provides a `/retcon:rewrite-git-history` slash command
//! that guides the user through creating a history specification.

use determinishtic::Determinishtic;
use sacp::mcp_server::{McpConnectionTo, McpServer};
use sacp::schema::{
    AvailableCommand, ContentBlock, PromptRequest, SessionNotification, SessionUpdate, TextContent,
};
use sacp::{Agent, Client, Conductor, Proxy};
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
    // 1. Write TOML to temp file
    let temp_dir =
        tempfile::tempdir().map_err(|e| sacp::Error::internal_error().data(e.to_string()))?;
    let spec_path = temp_dir.path().join("spec.toml");
    std::fs::write(&spec_path, &params.toml_spec)
        .map_err(|e| sacp::Error::internal_error().data(e.to_string()))?;

    // 2. Build config
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

    // 3. Create Determinishtic from the existing connection
    let connection = cx.connection_to();
    let d = Determinishtic::from_connection(connection);

    // 4. Run execute
    let result = retcon::execute_with_connection(&d, &spec_path, &config).await;

    // 5. Read updated TOML
    let updated_toml = std::fs::read_to_string(&spec_path)
        .map_err(|e| sacp::Error::internal_error().data(e.to_string()))?;

    // 6. Parse to determine status
    let spec = retcon::HistorySpec::from_toml(&updated_toml)
        .map_err(|e| sacp::Error::internal_error().data(e.to_string()))?;

    let status = match result {
        Ok(()) => {
            // Check if all commits are complete
            if spec.commits.iter().all(|c| c.is_complete()) {
                ExecuteStatus::Complete
            } else {
                // Find stuck commit
                if let Some((idx, commit)) =
                    spec.commits.iter().enumerate().find(|(_, c)| c.is_stuck())
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
                }
            }
        }
        Err(e) => ExecuteStatus::Error {
            message: e.to_string(),
        },
    };

    Ok(ExecuteResult {
        status,
        updated_toml,
    })
}
