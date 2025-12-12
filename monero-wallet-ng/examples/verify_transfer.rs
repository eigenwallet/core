//! Example: Verify a mainnet transfer using a view key.
//!
//! Usage: cargo run --example mainnet_verify_transfer -- [node_url]
//! Example: cargo run --example mainnet_verify_transfer -- http://node.moneroworld.com:18089
use monero_interface::ProvidesBlockchainMeta;
use monero_oxide_wallet::ed25519::{CompressedPoint, Scalar};
use monero_oxide_wallet::interface::ProvidesTransactions;
use monero_simple_request_rpc::SimpleRequestTransport;
use zeroize::Zeroizing;

use monero_wallet_ng::confirmations::{get_confirmations, ConfirmationStatus};
use monero_wallet_ng::rpc::ProvidesTransactionStatus;
use monero_wallet_ng::verify::verify_transfer;

const DEFAULT_NODE_URL: &str = "http://xmr-node.cakewallet.com:18081";

/// Transaction ID to verify
/// https://xmrchain.net/tx/fdd9ce7194cd8e6d14ecf7c5f49e7882be7248cde26215bb0bd9e20de1791b8e
const TX_ID_HEX: &str = "fdd9ce7194cd8e6d14ecf7c5f49e7882be7248cde26215bb0bd9e20de1791b8e";

/// Secret view key
const SECRET_VIEW_KEY_HEX: &str =
    "fd2dcdbfd47d9ef60aeaa5cc597822c41c6b4f4a0f5e5277f29c860989a2d209";

/// Public spend key
const PUBLIC_SPEND_KEY_HEX: &str =
    "1031c07d9b5772d14797826587d23135f2495a2bf173a5ae802f90d8fd1625be";

/// Expected amount (0.001 XMR)
const EXPECTED_AMOUNT: u64 = 1_000_000_000;

fn hex_to_bytes<const N: usize>(hex: &str) -> [u8; N] {
    let mut bytes = [0u8; N];
    hex::decode_to_slice(hex, &mut bytes).expect("invalid hex");
    bytes
}

async fn print_confirmation_status<P>(daemon: &P, tx_id: [u8; 32])
where
    P: ProvidesTransactionStatus + ProvidesBlockchainMeta,
{
    let confirmation_status = get_confirmations(daemon, tx_id)
        .await
        .expect("failed to get confirmations");

    match confirmation_status {
        ConfirmationStatus::Unseen => {
            println!("Transaction not found");
        }
        ConfirmationStatus::InPool => {
            println!("Transaction is in the mempool (unconfirmed)");
        }
        ConfirmationStatus::Confirmed { confirmations } => {
            println!("Transaction has {} confirmations", confirmations);
        }
    }
}

async fn verify_and_print_transfer<P>(daemon: &P, tx_id: [u8; 32])
where
    P: ProvidesTransactions,
{
    // Public spend key
    let public_spend_key_bytes: [u8; 32] = hex_to_bytes(PUBLIC_SPEND_KEY_HEX);
    let public_spend_key = CompressedPoint::from(public_spend_key_bytes)
        .decompress()
        .expect("invalid public spend key");

    // Secret view key
    let secret_view_key_bytes: [u8; 32] = hex_to_bytes(SECRET_VIEW_KEY_HEX);
    let private_view_key = Zeroizing::new(
        Scalar::read(&mut secret_view_key_bytes.as_slice()).expect("invalid scalar"),
    );

    println!("Verifying transfer...");
    println!("TX ID: {}", TX_ID_HEX);
    println!("Expected amount: {} piconero", EXPECTED_AMOUNT);

    let result = verify_transfer(
        daemon,
        tx_id,
        public_spend_key,
        private_view_key,
        EXPECTED_AMOUNT,
    )
    .await
    .expect("failed to verify transfer");

    assert!(result, "transfer verification failed");

    println!("Transfer verified!");
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let node_url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| DEFAULT_NODE_URL.to_string());

    let daemon = SimpleRequestTransport::new(node_url)
        .await
        .expect("failed to create RPC client");

    // Transaction to verify
    let tx_id: [u8; 32] = hex_to_bytes(TX_ID_HEX);

    print_confirmation_status(&daemon, tx_id).await;
    verify_and_print_transfer(&daemon, tx_id).await;

    Ok(())
}
