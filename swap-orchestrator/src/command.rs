mod build;
mod export;
mod init;
pub mod prompt;
mod start;

pub use build::build;
pub use export::export;
pub use init::init;
pub use start::start;

/// Top level args to the orchestrator cli.
/// Fields in here can/must always be specified.
#[derive(clap::Parser)]
pub struct Args {
    #[arg(
        long,
        default_value = "false",
        long_help = "Specify this flag when you want to run on Bitcoin Testnet and Monero Stagenet. Mainly used for development."
    )]
    pub testnet: bool,
    /// The actual command to execute.
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(clap::Subcommand)]
pub enum Command {
    Init,
    Start,
    Build,
    Export,
}
