use std::path::PathBuf;
use std::sync::Arc;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "retcon")]
#[command(about = "Reconstruct clean git history from messy branches")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Emit LLM guidance for creating a history specification
    Prompt,

    /// Execute the reconstruction from a history specification
    Execute {
        /// Path to the history specification TOML file
        plan: PathBuf,

        /// Build command to run after each commit (default: cargo check --all --workspace)
        #[arg(long)]
        build_command: Option<String>,

        /// Test command to run after build passes (default: cargo test --all --workspace)
        #[arg(long)]
        test_command: Option<String>,

        /// Skip build or test step (can be specified multiple times)
        #[arg(long = "skip", value_name = "STEP")]
        skip: Vec<SkipStep>,

        /// Disable the TUI and use plain text output
        #[arg(long)]
        no_tui: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
enum SkipStep {
    Build,
    Test,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Prompt => {
            print!("{}", retcon::prompt());
        }
        Command::Execute {
            plan,
            build_command,
            test_command,
            skip,
            no_tui,
        } => {
            let config = retcon::ExecuteConfig {
                build_command: if skip.contains(&SkipStep::Build) {
                    None
                } else {
                    Some(
                        build_command
                            .unwrap_or_else(|| "cargo check --all --workspace".to_string()),
                    )
                },
                test_command: if skip.contains(&SkipStep::Test) {
                    None
                } else {
                    Some(test_command.unwrap_or_else(|| "cargo test --all --workspace".to_string()))
                },
            };

            if no_tui {
                retcon::execute(&plan, &config).await?;
            } else {
                run_with_tui(plan, config).await?;
            }
        }
    }

    Ok(())
}

async fn run_with_tui(plan: PathBuf, config: retcon::ExecuteConfig) -> anyhow::Result<()> {
    let (app, observer, hooks) = retcon::tui::TuiApp::new();

    // We need to share `app` between the TUI thread and the async task
    let app_handle = Arc::new(app);
    let app_for_task = app_handle.clone();

    // Spawn the execute loop in a background task
    let execute_handle = tokio::spawn(async move {
        let result =
            retcon::execute_with_hooks(&plan, &config, &hooks, Some(Arc::new(observer))).await;

        // Signal the TUI that we're done
        match &result {
            Ok(()) => app_for_task.signal_done(Ok(())),
            Err(e) => app_for_task.signal_done(Err(e.to_string())),
        }

        result
    });

    // Run the TUI on the main thread (ratatui needs it for terminal control).
    // Use spawn_blocking so we don't block the tokio runtime.
    let tui_result = tokio::task::spawn_blocking(move || app_handle.run()).await?;

    // Wait for the execute task to finish
    let execute_result = execute_handle.await?;

    // Report TUI errors
    tui_result?;

    // Report execute errors
    execute_result?;

    Ok(())
}
