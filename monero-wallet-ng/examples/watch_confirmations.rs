use std::time::Duration;

use monero_simple_request_rpc::SimpleRequestTransport;

use monero_wallet_ng::confirmations::subscribe;

fn hex_to_bytes<const N: usize>(hex: &str) -> Result<[u8; N], hex::FromHexError> {
    let mut bytes = [0u8; N];
    hex::decode_to_slice(hex, &mut bytes)?;
    Ok(bytes)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() != 3 {
        eprintln!("Usage: {} <node_url> <tx_id_hex>", args[0]);
        eprintln!(
            "Example: {} http://node.moneroworld.com:18089 fdd9ce7194cd8e6d14ecf7c5f49e7882be7248cde26215bb0bd9e20de1791b8e",
            args[0]
        );
        std::process::exit(1);
    }

    let node_url = &args[1];
    let tx_id_hex = &args[2];

    let tx_id: [u8; 32] = hex_to_bytes(tx_id_hex).expect("tx_id must be a 64 character hex string");

    let daemon = SimpleRequestTransport::new(node_url.clone())
        .await
        .expect("failed to create RPC client");

    let subscription = subscribe(daemon, tx_id, Duration::from_secs(5), Default::default());
    let mut receiver = subscription.receiver;

    println!("Watching transaction {}...", tx_id_hex);

    loop {
        println!("{:?}", *receiver.borrow());
        if receiver.changed().await.is_err() {
            break;
        }
    }

    Ok(())
}
