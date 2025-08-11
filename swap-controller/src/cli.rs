use clap::{Parser, Subcommand};

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
    /// Get Monero balance
    MoneroBalance,
    /// Get Monero wallet address
    MoneroAddress,
    /// Get Monero seed and restore height
    MoneroSeed,
    /// Get external multiaddresses
    Multiaddresses,
    /// Get active connection count
    ActiveConnections,
    /// Get list of swaps
    GetSwaps,
}
