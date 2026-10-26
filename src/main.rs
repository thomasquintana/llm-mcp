//! Command-line entry point for llm-mcp.

use clap::{Parser, Subcommand};
use llm_mcp::{
    auth,
    config::Config,
    error::{Error, Result},
};
use std::path::PathBuf;

#[derive(Parser)]
#[command(version, about = "OpenAI generation and decisions over MCP")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Serve MCP using environment configuration.
    Serve,
    /// Continue with ChatGPT in a browser.
    Login,
    /// Remove local OAuth credentials.
    Logout,
    /// Inspect or import authentication state.
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
    /// Export exact MCP descriptors and identity.
    Catalog,
}

#[derive(Subcommand)]
enum AuthCommand {
    /// Show token-free local login status.
    Status,
    /// Import transferred credentials, preserving destination host identity.
    Import { source: PathBuf },
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "llm_mcp=info".into()),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();
    match run(Cli::parse()).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<()> {
    let command = cli.command.unwrap_or(Command::Serve);
    if matches!(command, Command::Catalog) {
        println!(
            "{}",
            serde_json::to_string_pretty(&llm_mcp::mcp::catalog()).expect("static catalog")
        );
        return Ok(());
    }
    let config = Config::from_env()?;
    match command {
        Command::Serve => Err(Error::new(
            "configuration",
            "transport wiring is not initialized",
        )),
        Command::Login => auth::login(&config.credentials_path).await,
        Command::Logout => auth::logout(&config.credentials_path),
        Command::Auth {
            command: AuthCommand::Status,
        } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&auth::status(&config.credentials_path)?)
                    .expect("status serializes")
            );
            Ok(())
        }
        Command::Auth {
            command: AuthCommand::Import { source },
        } => auth::import(&config.credentials_path, &source),
        Command::Catalog => unreachable!("handled above"),
    }
}
