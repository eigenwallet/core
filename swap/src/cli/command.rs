use crate::cli::api::request::{
    BalanceArgs, CancelAndRefundArgs, ExportBitcoinWalletArgs, GetConfigArgs, GetHistoryArgs,
    MoneroRecoveryArgs, Request, ResumeSwapArgs, WithdrawBtcArgs,
};
use crate::cli::api::Context;
use crate::monero::{self, MoneroAddressPool};
use anyhow::Result;
use bitcoin::address::NetworkUnchecked;
use bitcoin_wallet::{bitcoin_address, Amount};
use libp2p::core::Multiaddr;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use structopt::{clap, StructOpt};
use url::Url;
use uuid::Uuid;

use super::api::request::GetLogsArgs;
use super::api::ContextBuilder;

// See: https://1209k.com/bitcoin-eye/ele.php?chain=btc
const DEFAULT_ELECTRUM_RPC_URL: &str = "ssl://blockstream.info:700";
// See: https://1209k.com/bitcoin-eye/ele.php?chain=tbtc
pub const DEFAULT_ELECTRUM_RPC_URL_TESTNET: &str = "tcp://electrum.blockstream.info:60001";

const DEFAULT_BITCOIN_CONFIRMATION_TARGET: u16 = 1;
pub const DEFAULT_BITCOIN_CONFIRMATION_TARGET_TESTNET: u16 = 1;

/// Represents the result of parsing the command-line parameters.

#[derive(Debug)]
pub enum ParseResult {
    /// The arguments we were invoked in.
    Success(Arc<Context>),
    /// A flag or command was given that does not need further processing other
    /// than printing the provided message.
    ///
    /// The caller should exit the program with exit code 0.
    PrintAndExitZero { message: String },
}

pub async fn parse_args_and_apply_defaults<I, T>(raw_args: I) -> Result<ParseResult>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    parse_args_and_custom(raw_args, apply_defaults).await
}

type JsonTestnetData = (bool, bool, Option<PathBuf>);
async fn parse_args_and_custom<I, T, Hcc>(
    raw_args: I,
    handle_cli_command: impl FnOnce(CliCommand, Arc<Context>, JsonTestnetData) -> Hcc,
) -> Result<ParseResult>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
    Hcc: std::future::Future<Output = Result<()>>,
{
    let args = match Arguments::clap().get_matches_from_safe(raw_args) {
        Ok(matches) => Arguments::from_clap(&matches),
        Err(clap::Error {
            message,
            kind: clap::ErrorKind::HelpDisplayed | clap::ErrorKind::VersionDisplayed,
            ..
        }) => return Ok(ParseResult::PrintAndExitZero { message }),
        Err(e) => anyhow::bail!(e),
    };

    let context = Arc::new(Context::new_without_tauri_handle());
    handle_cli_command(
        args.cmd,
        context.clone(),
        (args.json, args.testnet, args.data),
    )
    .await?;
    Ok(ParseResult::Success(context))
}

async fn apply_defaults(
    cli_cmd: CliCommand,
    context: Arc<Context>,
    (json, is_testnet, data): JsonTestnetData,
) -> Result<()> {
    match cli_cmd {
        CliCommand::History => {
            ContextBuilder::new(is_testnet)
                .with_data_dir(data)
                .with_json(json)
                .build(context.clone())
                .await?;

            GetHistoryArgs {}.request(context).await?;
        }
        CliCommand::Logs {
            logs_dir,
            redact,
            swap_id,
        } => {
            ContextBuilder::new(is_testnet)
                .with_data_dir(data)
                .with_json(json)
                .build(context.clone())
                .await?;

            GetLogsArgs {
                logs_dir,
                redact,
                swap_id,
            }
            .request(context)
            .await?;
        }
        CliCommand::Config => {
            ContextBuilder::new(is_testnet)
                .with_data_dir(data)
                .with_json(json)
                .build(context.clone())
                .await?;

            GetConfigArgs {}.request(context).await?;
        }
        CliCommand::Balance { bitcoin } => {
            ContextBuilder::new(is_testnet)
                .with_bitcoin(bitcoin)
                .with_data_dir(data)
                .with_json(json)
                .build(context.clone())
                .await?;

            BalanceArgs {
                force_refresh: true,
            }
            .request(context)
            .await?;
        }
        CliCommand::WithdrawBtc {
            bitcoin,
            amount,
            address,
        } => {
            let address = bitcoin_address::validate(address, is_testnet)?;

            ContextBuilder::new(is_testnet)
                .with_bitcoin(bitcoin)
                .with_data_dir(data)
                .with_json(json)
                .build(context.clone())
                .await?;

            WithdrawBtcArgs { amount, address }.request(context).await?;
        }
        CliCommand::Resume {
            swap_id: SwapId { swap_id },
            bitcoin,
            monero,
            tor,
        } => {
            ContextBuilder::new(is_testnet)
                .with_tor(tor.enable_tor)
                .with_bitcoin(bitcoin)
                .with_monero(monero)
                .with_data_dir(data)
                .with_json(json)
                .build(context.clone())
                .await?;

            ResumeSwapArgs { swap_id }.request(context).await?;
        }
        CliCommand::CancelAndRefund {
            swap_id: SwapId { swap_id },
            bitcoin,
        } => {
            ContextBuilder::new(is_testnet)
                .with_bitcoin(bitcoin)
                .with_data_dir(data)
                .with_json(json)
                .build(context.clone())
                .await?;

            CancelAndRefundArgs { swap_id }.request(context).await?;
        }
        CliCommand::ExportBitcoinWallet { bitcoin } => {
            ContextBuilder::new(is_testnet)
                .with_bitcoin(bitcoin)
                .with_data_dir(data)
                .with_json(json)
                .build(context.clone())
                .await?;

            ExportBitcoinWalletArgs {}.request(context).await?;
        }
        CliCommand::MoneroRecovery {
            swap_id: SwapId { swap_id },
        } => {
            ContextBuilder::new(is_testnet)
                .with_data_dir(data)
                .with_json(json)
                .build(context.clone())
                .await?;

            MoneroRecoveryArgs { swap_id }.request(context).await?;
        }
    }
    Ok(())
}

#[derive(structopt::StructOpt, Debug)]
#[structopt(
    name = "swap",
    about = "CLI for swapping BTC for XMR",
    author,
    version = env!("CARGO_PKG_VERSION")
)]
struct Arguments {
    // global is necessary to ensure that clap can match against testnet in subcommands
    #[structopt(
        long,
        help = "Swap on testnet and assume testnet defaults for data-dir and the blockchain related parameters",
        global = true
    )]
    testnet: bool,

    #[structopt(
        short,
        long = "--data-base-dir",
        help = "The base data directory to be used for mainnet / testnet specific data like database, wallets etc"
    )]
    data: Option<PathBuf>,

    #[structopt(
        short,
        long = "json",
        help = "Outputs all logs in JSON format instead of plain text"
    )]
    json: bool,

    #[structopt(subcommand)]
    cmd: CliCommand,
}

#[derive(structopt::StructOpt, Debug, PartialEq)]
enum CliCommand {
    /// Show a list of past, ongoing and completed swaps
    History,
    /// Output all logging messages that have been issued.
    Logs {
        #[structopt(
            short = "d",
            help = "Print the logs from this directory instead of the default one."
        )]
        logs_dir: Option<PathBuf>,
        #[structopt(
            help = "Redact swap-ids, Bitcoin and Monero addresses.",
            long = "redact"
        )]
        redact: bool,
        #[structopt(
            long = "swap-id",
            help = "Filter for logs concerning this swap.",
            long_help = "This checks whether each logging message contains the swap id. Some messages might be skipped when they don't contain the swap id even though they're relevant."
        )]
        swap_id: Option<Uuid>,
    },
    #[structopt(about = "Prints the current config")]
    Config,
    #[structopt(about = "Allows withdrawing BTC from the internal Bitcoin wallet.")]
    WithdrawBtc {
        #[structopt(flatten)]
        bitcoin: Bitcoin,

        #[structopt(
            long = "amount",
            help = "Optionally specify the amount of Bitcoin to be withdrawn. If not specified the wallet will be drained."
        )]
        amount: Option<Amount>,

        #[structopt(long = "address",
            help = "The address to receive the Bitcoin.",
            parse(try_from_str = bitcoin_address::parse)
        )]
        address: bitcoin::Address<NetworkUnchecked>,
    },
    #[structopt(about = "Prints the Bitcoin balance.")]
    Balance {
        #[structopt(flatten)]
        bitcoin: Bitcoin,
    },
    /// Resume a swap
    Resume {
        #[structopt(flatten)]
        swap_id: SwapId,

        #[structopt(flatten)]
        bitcoin: Bitcoin,

        #[structopt(flatten)]
        monero: Monero,

        #[structopt(flatten)]
        tor: Tor,
    },
    /// Force the submission of the cancel and refund transactions of a swap
    #[structopt(aliases = &["cancel", "refund"])]
    CancelAndRefund {
        #[structopt(flatten)]
        swap_id: SwapId,

        #[structopt(flatten)]
        bitcoin: Bitcoin,
    },
    /// Print the internal bitcoin wallet descriptor
    ExportBitcoinWallet {
        #[structopt(flatten)]
        bitcoin: Bitcoin,
    },
    /// Prints Monero information related to the swap in case the generated
    /// wallet fails to detect the funds. This can only be used for swaps
    /// that are in a `btc is redeemed` state.
    MoneroRecovery {
        #[structopt(flatten)]
        swap_id: SwapId,
    },
}

#[derive(structopt::StructOpt, Debug, PartialEq, Default)]
pub struct Monero {
    #[structopt(
        long = "monero-node-address",
        help = "Specify to connect to a monero node of your choice: <host>:<port>"
    )]
    pub monero_node_address: Option<Url>,
}

#[derive(structopt::StructOpt, Debug, PartialEq, Default)]
pub struct Bitcoin {
    #[structopt(long = "electrum-rpc", help = "Provide the Bitcoin Electrum RPC URLs")]
    pub bitcoin_electrum_rpc_urls: Vec<String>,

    #[structopt(
        long = "bitcoin-target-block",
        help = "Estimate Bitcoin fees such that transactions are confirmed within the specified number of blocks"
    )]
    pub bitcoin_target_block: Option<u16>,
}

impl Bitcoin {
    pub fn apply_defaults(self, testnet: bool) -> Result<(Vec<String>, u16)> {
        let bitcoin_electrum_rpc_urls = if !self.bitcoin_electrum_rpc_urls.is_empty() {
            self.bitcoin_electrum_rpc_urls
        } else if testnet {
            vec![DEFAULT_ELECTRUM_RPC_URL_TESTNET.to_string()]
        } else {
            vec![DEFAULT_ELECTRUM_RPC_URL.to_string()]
        };

        let bitcoin_target_block = if let Some(target_block) = self.bitcoin_target_block {
            target_block
        } else if testnet {
            DEFAULT_BITCOIN_CONFIRMATION_TARGET_TESTNET
        } else {
            DEFAULT_BITCOIN_CONFIRMATION_TARGET
        };

        Ok((bitcoin_electrum_rpc_urls, bitcoin_target_block))
    }
}

#[derive(structopt::StructOpt, Debug, PartialEq, Default)]
pub struct Tor {
    #[structopt(
        long = "enable-tor",
        help = "Bootstrap a tor client and use it for all libp2p connections"
    )]
    pub enable_tor: bool,
}

#[derive(structopt::StructOpt, Debug, PartialEq)]
struct SwapId {
    #[structopt(
        long = "swap-id",
        help = "The swap id can be retrieved using the history subcommand"
    )]
    swap_id: Uuid,
}

#[derive(structopt::StructOpt, Debug, PartialEq)]
struct Seller {
    #[structopt(
        long,
        help = "The seller's address. Must include a peer ID part, i.e. `/p2p/`"
    )]
    seller: Multiaddr,
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::cli::api::api_test::*;
    use swap_serde::monero::address::MoneroAddressNetworkMismatch;

    const BINARY_NAME: &str = "swap";
    const ARGS_DATA_DIR: &str = "/tmp/dir/";

    async fn simple_positive(
        raw_ars: &[&str],
        want_json_testnet_data: JsonTestnetData,
        want_cli_cmd: CliCommand,
    ) {
        parse_args_and_custom(raw_ars, async |cli_cmd, _, json_testnet_data| {
            assert_eq!(json_testnet_data, want_json_testnet_data);
            assert_eq!(cli_cmd, want_cli_cmd);
            Ok(())
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn given_buy_xmr_on_mainnet_with_testnet_address_then_fails() {
        let raw_ars = [
            BINARY_NAME,
            "buy-xmr",
            "--receive-address",
            MONERO_STAGENET_ADDRESS,
            "--change-address",
            BITCOIN_TESTNET_ADDRESS,
            "--seller",
            MULTI_ADDRESS,
        ];

        let err = parse_args_and_apply_defaults(raw_ars).await.unwrap_err();
        assert_eq!(
            err.downcast_ref::<MoneroAddressNetworkMismatch>().unwrap(),
            &MoneroAddressNetworkMismatch {
                expected: monero::Network::Mainnet,
                actual: monero::Network::Stagenet
            }
        );
    }

    #[tokio::test]
    async fn given_buy_xmr_on_testnet_with_mainnet_address_then_fails() {
        let raw_ars = [
            BINARY_NAME,
            "--testnet",
            "buy-xmr",
            "--receive-address",
            MONERO_MAINNET_ADDRESS,
            "--change-address",
            BITCOIN_MAINNET_ADDRESS,
            "--seller",
            MULTI_ADDRESS,
        ];

        let err = parse_args_and_apply_defaults(raw_ars).await.unwrap_err();
        assert_eq!(
            err.downcast_ref::<MoneroAddressNetworkMismatch>().unwrap(),
            &MoneroAddressNetworkMismatch {
                expected: monero::Network::Stagenet,
                actual: monero::Network::Mainnet
            }
        );
    }

    #[tokio::test]
    async fn given_resume_on_mainnet_then_defaults_to_mainnet() {
        let raw_ars = [BINARY_NAME, "resume", "--swap-id", SWAP_ID];
        let cli_cmd = CliCommand::Resume {
            swap_id: SwapId {
                swap_id: SWAP_ID.parse().unwrap(),
            },
            bitcoin: Default::default(),
            monero: Default::default(),
            tor: Default::default(),
        };
        simple_positive(&raw_ars, (false, false, None), cli_cmd).await;
    }

    #[tokio::test]
    async fn given_resume_on_testnet_then_defaults_to_testnet() {
        let raw_ars = [BINARY_NAME, "--testnet", "resume", "--swap-id", SWAP_ID];
        let cli_cmd = CliCommand::Resume {
            swap_id: SwapId {
                swap_id: SWAP_ID.parse().unwrap(),
            },
            bitcoin: Default::default(),
            monero: Default::default(),
            tor: Default::default(),
        };
        simple_positive(&raw_ars, (false, true, None), cli_cmd).await;
    }

    #[tokio::test]
    async fn given_cancel_on_mainnet_then_defaults_to_mainnet() {
        for alias in ["cancel", "refund"] {
            let raw_ars = [BINARY_NAME, alias, "--swap-id", SWAP_ID];
            let cli_cmd = CliCommand::CancelAndRefund {
                swap_id: SwapId {
                    swap_id: SWAP_ID.parse().unwrap(),
                },
                bitcoin: Default::default(),
            };
            simple_positive(&raw_ars, (false, false, None), cli_cmd).await;
        }
    }

    #[tokio::test]
    async fn given_cancel_on_testnet_then_defaults_to_testnet() {
        for alias in ["cancel", "refund"] {
            let raw_ars = [BINARY_NAME, "--testnet", alias, "--swap-id", SWAP_ID];
            let cli_cmd = CliCommand::CancelAndRefund {
                swap_id: SwapId {
                    swap_id: SWAP_ID.parse().unwrap(),
                },
                bitcoin: Default::default(),
            };
            simple_positive(&raw_ars, (false, true, None), cli_cmd).await;
        }
    }

    #[tokio::test]
    async fn given_resume_on_mainnet_with_data_dir_then_data_dir_set() {
        let raw_ars = [
            BINARY_NAME,
            "--data-base-dir",
            ARGS_DATA_DIR,
            "resume",
            "--swap-id",
            SWAP_ID,
        ];
        let cli_cmd = CliCommand::Resume {
            swap_id: SwapId {
                swap_id: SWAP_ID.parse().unwrap(),
            },
            bitcoin: Default::default(),
            monero: Default::default(),
            tor: Default::default(),
        };
        simple_positive(
            &raw_ars,
            (false, false, Some(ARGS_DATA_DIR.into())),
            cli_cmd,
        )
        .await;
    }

    #[tokio::test]
    async fn given_resume_on_testnet_with_data_dir_then_data_dir_set() {
        let raw_ars = [
            BINARY_NAME,
            "--testnet",
            "--data-base-dir",
            ARGS_DATA_DIR,
            "resume",
            "--swap-id",
            SWAP_ID,
        ];
        let cli_cmd = CliCommand::Resume {
            swap_id: SwapId {
                swap_id: SWAP_ID.parse().unwrap(),
            },
            bitcoin: Default::default(),
            monero: Default::default(),
            tor: Default::default(),
        };
        simple_positive(&raw_ars, (false, true, Some(ARGS_DATA_DIR.into())), cli_cmd).await;
    }

    #[tokio::test]
    async fn given_resume_on_mainnet_with_json_then_json_set() {
        let raw_ars = [BINARY_NAME, "--json", "resume", "--swap-id", SWAP_ID];
        let cli_cmd = CliCommand::Resume {
            swap_id: SwapId {
                swap_id: SWAP_ID.parse().unwrap(),
            },
            bitcoin: Default::default(),
            monero: Default::default(),
            tor: Default::default(),
        };
        simple_positive(&raw_ars, (true, false, None), cli_cmd).await;
    }

    #[tokio::test]
    async fn given_resume_on_testnet_with_json_then_json_set() {
        let raw_ars = [
            BINARY_NAME,
            "--testnet",
            "--json",
            "resume",
            "--swap-id",
            SWAP_ID,
        ];
        let cli_cmd = CliCommand::Resume {
            swap_id: SwapId {
                swap_id: SWAP_ID.parse().unwrap(),
            },
            bitcoin: Default::default(),
            monero: Default::default(),
            tor: Default::default(),
        };
        simple_positive(&raw_ars, (true, true, None), cli_cmd).await;
    }

    #[tokio::test]
    async fn only_bech32_addresses_mainnet_are_allowed() {
        // TODO: not apply defaults
        let mut raw_ars = [
            BINARY_NAME,
            "buy-xmr",
            "--change-address",
            "",
            "--receive-address",
            MONERO_MAINNET_ADDRESS,
            "--seller",
            MULTI_ADDRESS,
        ];
        raw_ars[3] = "1A5btpLKZjgYm8R22rJAhdbTFVXgSRA2Mp";
        parse_args_and_custom(raw_ars, async |_, _, _| unreachable!())
            .await
            .unwrap_err();

        raw_ars[3] = "36vn4mFhmTXn7YcNwELFPxTXhjorw2ppu2";
        parse_args_and_custom(raw_ars, async |_, _, _| unreachable!())
            .await
            .unwrap_err();

        raw_ars[3] = "bc1qh4zjxrqe3trzg7s6m7y67q2jzrw3ru5mx3z7j3";
        let ParseResult::Success(_) = parse_args_and_custom(raw_ars, async |_, _, _| Ok(()))
            .await
            .unwrap()
        else {
            panic!()
        };
    }

    #[tokio::test]
    async fn only_bech32_addresses_testnet_are_allowed() {
        let mut raw_ars = [
            BINARY_NAME,
            "--testnet",
            "buy-xmr",
            "--change-address",
            "",
            "--receive-address",
            MONERO_STAGENET_ADDRESS,
            "--seller",
            MULTI_ADDRESS,
        ];
        raw_ars[4] = "n2czxyeFCQp9e8WRyGpy4oL4YfQAeKkkUH";
        parse_args_and_custom(raw_ars, async |_, _, _| unreachable!())
            .await
            .unwrap_err();
        raw_ars[4] = "2ND9a4xmQG89qEWG3ETRuytjKpLmGrW7Jvf";
        parse_args_and_custom(raw_ars, async |_, _, _| unreachable!())
            .await
            .unwrap_err();

        raw_ars[4] = "tb1q958vfh3wkdp232pktq8zzvmttyxeqnj80zkz3v";
        let ParseResult::Success(_) = parse_args_and_custom(raw_ars, async |_, _, _| Ok(()))
            .await
            .unwrap()
        else {
            panic!()
        };
    }
}
