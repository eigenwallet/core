mod cli;
mod repl;

use clap::Parser;
use cli::{Cli, Cmd};
use swap_controller_api::AsbApiClient;

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
            println!("Current Bitcoin balance is {} BTC", response.balance);
        }
        Cmd::MoneroBalance => {
            let response = client.monero_balance().await?;
            let amount = monero::Amount::from_pico(response.balance);
            
            println!("Current Monero balance is {:.12} XMR", amount.as_xmr());
        }
    }
    Ok(())
}
