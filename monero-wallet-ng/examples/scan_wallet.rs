//! Example: Scan the blockchain for outputs using a view key.
//!
//! Usage: cargo run --example scan_wallet -- [node_url]
//! Example: cargo run --example scan_wallet -- http://node.moneroworld.com:18089

use std::time::Duration;

use monero_oxide_wallet::ed25519::{CompressedPoint, Scalar};
use monero_oxide_wallet::ViewPair;
use monero_simple_request_rpc::SimpleRequestTransport;
use zeroize::Zeroizing;

use monero_wallet_ng::scanner;

const DEFAULT_NODE_URL: &str = "http://xmr-node.cakewallet.com:18081";
const RESTORE_HEIGHT: usize = 3562000;
const POLL_INTERVAL: Duration = Duration::from_secs(10);

/// Secret view key
const SECRET_VIEW_KEY_HEX: &str =
    "fd2dcdbfd47d9ef60aeaa5cc597822c41c6b4f4a0f5e5277f29c860989a2d209";

/// Public spend key
const PUBLIC_SPEND_KEY_HEX: &str =
    "1031c07d9b5772d14797826587d23135f2495a2bf173a5ae802f90d8fd1625be";

fn hex_to_bytes<const N: usize>(hex: &str) -> [u8; N] {
    let mut bytes = [0u8; N];
    hex::decode_to_slice(hex, &mut bytes).expect("invalid hex");
    bytes
}

fn create_view_pair() -> ViewPair {
    let public_spend_key_bytes: [u8; 32] = hex_to_bytes(PUBLIC_SPEND_KEY_HEX);
    let public_spend_key = CompressedPoint::from(public_spend_key_bytes)
        .decompress()
        .expect("invalid public spend key");

    let secret_view_key_bytes: [u8; 32] = hex_to_bytes(SECRET_VIEW_KEY_HEX);
    let private_view_key = Zeroizing::new(
        Scalar::read(&mut secret_view_key_bytes.as_slice()).expect("invalid scalar"),
    );

    ViewPair::new(public_spend_key, private_view_key).expect("invalid view pair")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let node_url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| DEFAULT_NODE_URL.to_string());

    let daemon = SimpleRequestTransport::new(node_url).await?;
    let view_pair = create_view_pair();

    let mut subscription = scanner::scanner(daemon, view_pair, RESTORE_HEIGHT, POLL_INTERVAL);

    while let Some(output) = subscription.outputs.recv().await {
        println!(
            "Found output: tx={} amount={} piconero",
            hex::encode(output.transaction()),
            output.commitment().amount
        );
    }

    Ok(())
}
