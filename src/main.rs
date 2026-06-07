use std::net::SocketAddr;

use clap::{Parser, Subcommand};
use ownpager::cli_intake::{handle_cli_message, run_cli_message};
use ownpager::config::default_bind_addr;
use ownpager::telegram::{serve_telegram, serve_telegram_run};
use ownpager::terminal::run_terminal_command;
use ownpager::types::TerminalCommandRequest;
use ownpager::web_intake::serve_web;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "ownpager")]
#[command(about = "Hermes-like query intake that stops before ThinkingRoot prepare_turn")]
struct Args {
    #[arg(long, global = true)]
    workspace: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Cli {
        #[arg(long)]
        message: String,

        #[arg(long, default_value = "default")]
        profile: String,
    },
    RunCli {
        #[arg(long)]
        message: String,

        #[arg(long, default_value = "default")]
        profile: String,
    },
    ServeTelegram {
        #[arg(long, default_value_t = default_bind_addr())]
        bind: SocketAddr,
    },
    ServeTelegramRun {
        #[arg(long, default_value_t = default_bind_addr())]
        bind: SocketAddr,
    },
    ServeWeb {
        #[arg(long, default_value_t = default_bind_addr())]
        bind: SocketAddr,
    },
    Terminal {
        #[arg(long)]
        command: String,

        #[arg(long)]
        workdir: Option<String>,

        #[arg(long)]
        timeout_seconds: Option<u64>,

        #[arg(long)]
        max_output_bytes: Option<usize>,

        #[arg(long)]
        allowed_root: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let args = Args::parse();

    match args.command {
        Command::Cli { message, profile } => {
            let preview = handle_cli_message(message, profile, args.workspace)?;
            println!("{}", serde_json::to_string_pretty(&preview)?);
        }
        Command::RunCli { message, profile } => {
            let result = run_cli_message(message, profile, args.workspace)
                .await
                .map_err(boxed_error)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::ServeTelegram { bind } => {
            serve_telegram(bind, args.workspace).await?;
        }
        Command::ServeTelegramRun { bind } => {
            serve_telegram_run(bind, args.workspace).await?;
        }
        Command::ServeWeb { bind } => {
            serve_web(bind, args.workspace).await?;
        }
        Command::Terminal {
            command,
            workdir,
            timeout_seconds,
            max_output_bytes,
            allowed_root,
        } => {
            let result = run_terminal_command(TerminalCommandRequest {
                command,
                workdir,
                timeout_seconds,
                max_output_bytes,
                allowed_root,
            });
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
    }

    Ok(())
}

fn boxed_error(message: String) -> Box<dyn std::error::Error + Send + Sync> {
    message.into()
}
