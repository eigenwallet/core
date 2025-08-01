use arti_client::{TorClient, TorClientConfig};
use clap::Parser;
use monero_rpc_pool::{config::Config, run_server};
use tracing::info;
use tracing_subscriber::{self, EnvFilter};

use monero::Network;

fn parse_network(s: &str) -> Result<Network, String> {
    match s.to_lowercase().as_str() {
        "mainnet" => Ok(Network::Mainnet),
        "stagenet" => Ok(Network::Stagenet),
        "testnet" => Ok(Network::Testnet),
        _ => Err(format!(
            "Invalid network: {}. Must be mainnet, stagenet, or testnet",
            s
        )),
    }
}

// TODO: Replace with Display impl for Network
fn network_to_string(network: &Network) -> String {
    match network {
        Network::Mainnet => "mainnet".to_string(),
        Network::Stagenet => "stagenet".to_string(),
        Network::Testnet => "testnet".to_string(),
    }
}

#[derive(Parser)]
#[command(name = "monero-rpc-pool")]
#[command(about = "A load-balancing HTTP proxy for Monero RPC nodes")]
#[command(version)]
struct Args {
    #[arg(long, default_value = "127.0.0.1")]
    #[arg(help = "Host address to bind the server to")]
    host: String,

    #[arg(short, long, default_value = "18081")]
    #[arg(help = "Port to bind the server to")]
    port: u16,

    #[arg(short, long, default_value = "mainnet")]
    #[arg(help = "Network to use for automatic node discovery")]
    #[arg(value_parser = parse_network)]
    network: Network,

    #[arg(short, long)]
    #[arg(help = "Enable verbose logging")]
    verbose: bool,

    #[arg(short, long)]
    #[arg(help = "Enable Tor routing")]
    #[arg(default_value = "true")]
    tor: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new("info"))
        .with_target(false)
        .with_file(true)
        .with_line_number(true)
        .init();

    let tor_client = if args.tor {
        let config = TorClientConfig::default();
        let runtime = tor_rtcompat::tokio::TokioRustlsRuntime::current()
            .expect("We are always running with tokio");

        let client = TorClient::with_runtime(runtime)
            .config(config)
            .create_unbootstrapped_async()
            .await?;

        let client = std::sync::Arc::new(client);

        let client_clone = client.clone();
        tokio::spawn(async move {
            match client_clone.bootstrap().await {
                Ok(()) => {
                    info!("Tor client successfully bootstrapped");
                }
                Err(e) => {
                    tracing::error!("Failed to bootstrap Tor client: {}. Tor functionality will be unavailable.", e);
                }
            }
        });

        Some(client)
    } else {
        None
    };

    let config = Config::new_with_port_and_tor_client(
        args.host,
        args.port,
        std::env::temp_dir().join("monero-rpc-pool"),
        tor_client,
    );

    info!(
        host = config.host,
        port = config.port,
        network = network_to_string(&args.network),
        "Starting Monero RPC Pool"
    );

    if let Err(e) = run_server(config, args.network).await {
        eprintln!("Server error: {}", e);
        std::process::exit(1);
    }

    Ok(())
}
