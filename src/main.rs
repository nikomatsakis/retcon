use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "herodotus")]
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
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Prompt => {
            print!("{}", herodotus::prompt());
        }
        Command::Execute { plan } => {
            herodotus::execute(&plan).await?;
        }
    }

    Ok(())
}
