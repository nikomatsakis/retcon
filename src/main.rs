use std::path::PathBuf;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use serde::Deserialize;

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

        /// Agent command to use for LLM work (e.g. "npx -y @zed-industries/claude-code-acp@latest")
        #[arg(long)]
        agent: Option<String>,

        /// Build command to run after each commit (default: cargo check --all --workspace)
        #[arg(long)]
        build_command: Option<String>,

        /// Test command to run after build passes (default: cargo test --all --workspace)
        #[arg(long)]
        test_command: Option<String>,

        /// Skip build or test step (can be specified multiple times)
        #[arg(long = "skip", value_name = "STEP")]
        skip: Vec<SkipStep>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
enum SkipStep {
    Build,
    Test,
}

/// Config loaded from ~/.retcon/config.toml
#[derive(Debug, Default, Deserialize)]
struct Config {
    #[serde(default)]
    agent: Option<String>,
}

fn load_config() -> Config {
    let Some(home) = dirs::home_dir() else {
        return Config::default();
    };
    let path = home.join(".retcon").join("config.toml");
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config_file = load_config();

    match cli.command {
        Command::Prompt => {
            print!("{}", retcon::prompt());
        }
        Command::Execute {
            plan,
            agent,
            build_command,
            test_command,
            skip,
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
                agent: agent.or(config_file.agent),
            };

            let (observer, hooks) = retcon::tui::new();
            retcon::execute_with_hooks(&plan, &config, &hooks, Some(Arc::new(observer))).await?;
        }
    }

    Ok(())
}
