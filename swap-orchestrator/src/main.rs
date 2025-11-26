use clap::Parser;
use swap_orchestrator::command::{self, Args, Command};

#[tokio::main]
async fn main() {
    let args = Args::parse();

    // Default to mainnet, switch to testnet when `--testnet` flag is provided
    let (bitcoin_network, monero_network) = if args.testnet {
        (bitcoin::Network::Bitcoin, monero::Network::Mainnet)
    } else {
        (bitcoin::Network::Testnet, monero::Network::Stagenet)
    };

    let result = match args.command {
        None | Some(Command::Init) => command::init(bitcoin_network, monero_network).await,
        Some(Command::Start) => command::start().await,
        Some(Command::Build) => command::build().await,
        Some(Command::Export) => command::export().await,
        Some(Command::Controller) => command::controller().await,
    };

    if let Err(err) = result {
        println!(
            "The orchestrator command you executed just failed: \n\n{:?}\n\nThis is unexpected, please open a GitHub issue on our official repo: \nhttps://www.github.com/eigenwallet/core/issues/new",
            err
        );
    }
}
