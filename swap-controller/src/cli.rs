use clap::{Parser, Subcommand};
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "asb-controller")]
#[command(about = "Control tool for ASB daemon")]
pub struct Cli {
    /// RPC server URL
    #[arg(long, default_value = "http://127.0.0.1:9944")]
    pub url: String,

    /// Command to execute (defaults to interactive shell if omitted)
    #[command(subcommand)]
    pub cmd: Option<Cmd>,
}

#[derive(Subcommand, Clone)]
pub enum Cmd {
    /// Check connection to ASB server
    CheckConnection,
    /// Get Bitcoin balance
    BitcoinBalance,
    /// Get Bitcoin descriptor containing private keys
    BitcoinSeed,
    /// Get Monero balance
    MoneroBalance,
    /// Get Monero wallet address
    MoneroAddress,
    /// Get Monero seed and restore height
    MoneroSeed,
    /// Get external multiaddresses
    Multiaddresses,
    /// Get peer ID
    PeerId,
    /// Get active connection count
    ActiveConnections,
    /// Get list of swaps
    GetSwaps,
    /// Show rendezvous registration status
    RegistrationStatus,
    /// Set whether to burn Bitcoin on refund for a swap
    SetWithholdDeposit {
        /// The swap ID
        swap_id: Uuid,
        /// Whether to burn the Bitcoin (true or false)
        #[arg(action = clap::ArgAction::Set)]
        withhold: bool,
    },
    /// Grant mercy (release the anti-spam deposit) for a swap in BtcWithheld state
    GrantMercy {
        /// The swap ID
        swap_id: Uuid,
    },
    /// Withdraw BTC from the internal Bitcoin wallet
    WithdrawBtc {
        /// The destination Bitcoin address
        address: String,
        /// Amount to withdraw, e.g. "0.1 BTC" or "10000 sat" (omit to sweep entire balance)
        amount: Option<bitcoin::Amount>,
    },
    /// Refresh the internal Bitcoin wallet by syncing with the blockchain
    RefreshBitcoinWallet,
    /// List active wormhole onion services
    WormholeServices,
    /// Show status of the primary onion service
    OnionServiceStatus,
}
