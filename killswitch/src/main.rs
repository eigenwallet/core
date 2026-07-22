use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use libc as _;
use reqwest as _;
use serde as _;
use serde_json as _;
#[cfg(test)]
use tempfile as _;
use tracing_subscriber::EnvFilter;
use url::Url;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    if let Err(error) = run().await {
        tracing::error!(error = ?error, "Killswitch stopped");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let mut arguments = std::env::args().skip(1);
    let Some(command) = arguments.next() else {
        bail!("Expected one of: monitor, health, supervise");
    };

    match command.as_str() {
        "monitor" => {
            let endpoint = std::env::var("KILLSWITCH_ENDPOINT")
                .unwrap_or_else(|_| killswitch::DEFAULT_ENDPOINT.to_string());
            let endpoint = Url::parse(&endpoint).context("Invalid KILLSWITCH_ENDPOINT")?;
            killswitch::monitor(endpoint, state_file()).await
        }
        "health" => killswitch::health(&state_file()).await,
        "supervise" => {
            let mut command: Vec<_> = arguments.collect();
            if command.first().is_some_and(|argument| argument == "--") {
                command.remove(0);
            }
            killswitch::supervise(state_file(), command).await
        }
        other => bail!("Unknown killswitch command `{other}`"),
    }
}

fn state_file() -> PathBuf {
    std::env::var_os("KILLSWITCH_STATE_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(killswitch::DEFAULT_STATE_FILE))
}
