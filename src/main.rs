use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "pravda")]
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
            print!("{}", pravda::prompt());
        }
        Command::Execute {
            plan,
            build_command,
            test_command,
            skip,
        } => {
            let config = pravda::ExecuteConfig {
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
                    Some(
                        test_command.unwrap_or_else(|| "cargo test --all --workspace".to_string()),
                    )
                },
            };
            pravda::execute(&plan, &config).await?;
        }
    }

    Ok(())
}
