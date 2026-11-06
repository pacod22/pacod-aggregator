use std::time::Duration;

use clap::{Parser, Subcommand};
use mimalloc::MiMalloc;
use pacod_agg::config::Config;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[derive(Parser)]
#[command(
    name = "pacod-agg",
    version,
    about = "pacod-aggregator's server: model data for AI gateways, read from the vendors' own pages"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run the server (the default).
    Serve,
    /// Migrate the database, and stop.
    Migrate,
    /// Print the OpenAPI document.
    Openapi,
    /// Exit 0 when `url` answers with a success: the image has no shell and no curl.
    Healthcheck { url: String },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command.unwrap_or(Command::Serve) {
        Command::Openapi => println!("{}", serde_json::to_string_pretty(&pacod_agg::openapi::document())?),
        Command::Healthcheck { url } => {
            let r = reqwest::Client::new().get(&url).timeout(Duration::from_secs(3)).send().await?;
            anyhow::ensure!(r.status().is_success(), "{url} answered {}", r.status());
        }
        Command::Migrate => {
            pacod_agg_logging::init("pacod-aggregator", "info", "AGG_LOG_FORMAT");
            let env = |n: &str| std::env::var(n).ok();
            let config = Config::from_env(&env)?;
            pacod_agg::connect(&config.database_url).await?.migrate().await?;
        }
        Command::Serve => {
            pacod_agg_logging::init("pacod-aggregator", "info", "AGG_LOG_FORMAT");
            let env = |n: &str| std::env::var(n).ok();
            pacod_agg::serve(Config::from_env(&env)?, signal()).await?;
        }
    }
    Ok(())
}

async fn signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = term => {}
    }
}
