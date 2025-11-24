mod cli;
mod repl;

use clap::Parser;
use cli::{Cli, Cmd};
use swap_controller_api::{AsbApiClient, MoneroSeedResponse};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let client = jsonrpsee::http_client::HttpClientBuilder::default().build(&cli.url)?;

    match cli.cmd {
        None => repl::run(client, dispatch).await?,
        Some(cmd) => {
            if let Err(e) = dispatch(cmd.clone(), client.clone()).await {
                eprintln!("Command failed with error: {e:?}");
            }
        }
    }

    Ok(())
}

async fn dispatch(cmd: Cmd, client: impl AsbApiClient) -> anyhow::Result<()> {
    match cmd {
        Cmd::CheckConnection => {
            client.check_connection().await?;
            println!("Connected");
        }
        Cmd::BitcoinBalance => {
            let response = client.bitcoin_balance().await?;
            println!("Current Bitcoin balance is {}", response.balance);
        }
        Cmd::MoneroBalance => {
            let response = client.monero_balance().await?;
            let amount = monero::Amount::from_pico(response.balance);

            println!("Current Monero balance is {:.12} XMR", amount.as_xmr());
        }
        Cmd::MoneroAddress => {
            let response = client.monero_address().await?;
            println!("The primary Monero address is {}", response.address);
        }
        Cmd::MoneroSeed => {
            let MoneroSeedResponse {
                seed,
                restore_height,
            } = client.monero_seed().await?;

            println!("The seed of the internal Monero wallet is: \n{}", seed);
            println!();
            println!(
                "The restore height of the internal Monero wallet is {}",
                restore_height
            );
        }
        Cmd::Multiaddresses => {
            let response = client.multiaddresses().await?;
            if response.multiaddresses.is_empty() {
                println!("No external multiaddresses configured");
            } else {
                for addr in response.multiaddresses {
                    println!("{}", addr);
                }
            }
        }
        Cmd::PeerId => {
            let response = client.peer_id().await?;
            println!("Peer IDs are used to identify peers within the P2P network.");
            println!("They are effectively the hash of your public key and are used for end-to-end encryption of network traffic.");
            println!();
            println!("Your Peer ID is: {}", response.peer_id);
        }
        Cmd::ActiveConnections => {
            let response = client.active_connections().await?;
            println!("Connected to {} peers", response.connections);
        }
        Cmd::GetSwaps => {
            let swaps = client.get_swaps().await?;
            if swaps.is_empty() {
                println!("No swaps found");
            } else {
                for swap in swaps {
                    println!("{}: {}", swap.id, swap.state);
                }
            }
        }
        Cmd::BitcoinSeed => {
            let response = client.bitcoin_seed().await?;
            println!("Descriptor (BIP-0382) containing the private keys of the internal Bitcoin wallet: \n{}", response.descriptor);
        }
        Cmd::RegistrationStatus => {
            let response = client.registration_status().await?;
            println!("Your asb registers at rendezvous to make itself discoverable to takers.\n");
            if response.registrations.is_empty() {
                println!("No rendezvous points configured");
            } else {
                for item in response.registrations {
                    let address = item.address.as_deref().unwrap_or("?");
                    println!(
                        "Connection status to rendezvous point at \"{}\" is \"{:?}\". Registration status is \"{:?}\"",
                        address, item.connection, item.registration
                    );
                }
            }
        }
    }
    Ok(())
}
