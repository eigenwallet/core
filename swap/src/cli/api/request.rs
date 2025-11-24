use super::tauri_bindings::TauriHandle;
use crate::bitcoin::wallet;
use crate::cli::api::tauri_bindings::{
    ApprovalRequestType, MoneroNodeConfig, SelectMakerDetails, SendMoneroDetails, TauriEmitter,
    TauriSwapProgressEvent,
};
use crate::cli::api::Context;
use crate::cli::list_sellers::{QuoteWithAddress, UnreachableSeller};
use crate::cli::SellerStatus;
use crate::common::{get_logs, redact};
use crate::monero::wallet_rpc::MoneroDaemon;
use crate::monero::MoneroAddressPool;
use crate::network::quote::BidQuote;
use crate::network::rendezvous::XmrBtcNamespace;
use crate::network::swarm;
use crate::protocol::bob::{self, BobState, Swap};
use crate::protocol::{Database, State};
use crate::{cli, monero};
use ::bitcoin::address::NetworkUnchecked;
use ::bitcoin::Txid;
use ::monero::Network;
use anyhow::{bail, Context as AnyContext, Result};
use arti_client::TorClient;
use futures::future::BoxFuture;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use libp2p::core::Multiaddr;
use libp2p::{identity, PeerId};
use monero_seed::{Language, Seed as MoneroSeed};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::convert::TryInto;
use std::future::Future;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use swap_core::bitcoin;
use swap_core::bitcoin::{CancelTimelock, ExpiredTimelocks, PunishTimelock};
use swap_p2p::libp2p_ext::MultiAddrExt;
use thiserror::Error;
use tokio_util::task::AbortOnDropHandle;
use tor_rtcompat::tokio::TokioRustlsRuntime;
use tracing::debug_span;
use tracing::error;
use tracing::Instrument;
use tracing::Span;
use typeshare::typeshare;
use url::Url;
use uuid::Uuid;
use zeroize::Zeroizing;

/// This trait is implemented by all types of request args that
/// the CLI can handle.
/// It provides a unified abstraction that can be useful for generics.
#[allow(async_fn_in_trait)]
pub trait Request {
    type Response: Serialize;
    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response>;
}

/// This generates a tracing span which is attached to all logs caused by a swap
fn get_swap_tracing_span(swap_id: Uuid) -> Span {
    debug_span!("swap", swap_id = %swap_id)
}

// BuyXmr
#[typeshare]
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BuyXmrArgs {
    #[typeshare(serialized_as = "Vec<string>")]
    pub sellers: Vec<Multiaddr>,
    #[typeshare(serialized_as = "Option<string>")]
    pub bitcoin_change_address: Option<bitcoin::Address<NetworkUnchecked>>,
    pub monero_receive_pool: MoneroAddressPool,
}

impl Request for BuyXmrArgs {
    type Response = ();

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        let swap_id = Uuid::new_v4();
        let swap_span = get_swap_tracing_span(swap_id);

        buy_xmr(self, swap_id, ctx).instrument(swap_span).await
    }
}

// ResumeSwap
#[typeshare]
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ResumeSwapArgs {
    #[typeshare(serialized_as = "string")]
    pub swap_id: Uuid,
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct ResumeSwapResponse {
    pub result: String,
}

impl Request for ResumeSwapArgs {
    type Response = ResumeSwapResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        let swap_span = get_swap_tracing_span(self.swap_id);

        resume_swap(self, ctx).instrument(swap_span).await
    }
}

// CancelAndRefund
#[typeshare]
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CancelAndRefundArgs {
    #[typeshare(serialized_as = "string")]
    pub swap_id: Uuid,
}

impl Request for CancelAndRefundArgs {
    type Response = serde_json::Value;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        let swap_span = get_swap_tracing_span(self.swap_id);

        cancel_and_refund(self, ctx).instrument(swap_span).await
    }
}

// MoneroRecovery
#[typeshare]
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MoneroRecoveryArgs {
    #[typeshare(serialized_as = "string")]
    pub swap_id: Uuid,
}

impl Request for MoneroRecoveryArgs {
    type Response = serde_json::Value;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        monero_recovery(self, ctx).await
    }
}

// WithdrawBtc
#[typeshare]
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WithdrawBtcArgs {
    #[typeshare(serialized_as = "number")]
    #[serde(default, with = "::bitcoin::amount::serde::as_sat::opt")]
    pub amount: Option<bitcoin::Amount>,
    #[typeshare(serialized_as = "string")]
    #[serde(with = "swap_serde::bitcoin::address_serde")]
    pub address: bitcoin::Address,
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct WithdrawBtcResponse {
    #[typeshare(serialized_as = "number")]
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    pub amount: bitcoin::Amount,
    pub txid: String,
}

impl Request for WithdrawBtcArgs {
    type Response = WithdrawBtcResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        withdraw_btc(self, ctx).await
    }
}

// GetSwapInfo
#[typeshare]
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GetSwapInfoArgs {
    #[typeshare(serialized_as = "string")]
    pub swap_id: Uuid,
}

#[typeshare]
#[derive(Serialize)]
pub struct GetSwapInfoResponse {
    #[typeshare(serialized_as = "string")]
    pub swap_id: Uuid,
    pub seller: AliceAddress,
    pub completed: bool,
    pub start_date: String,
    #[typeshare(serialized_as = "string")]
    pub state_name: String,
    #[typeshare(serialized_as = "number")]
    pub xmr_amount: monero::Amount,
    #[typeshare(serialized_as = "number")]
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    pub btc_amount: bitcoin::Amount,
    #[typeshare(serialized_as = "string")]
    pub tx_lock_id: Txid,
    #[typeshare(serialized_as = "number")]
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    pub tx_cancel_fee: bitcoin::Amount,
    #[typeshare(serialized_as = "number")]
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    pub tx_refund_fee: bitcoin::Amount,
    #[typeshare(serialized_as = "number")]
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    pub tx_lock_fee: bitcoin::Amount,
    pub btc_refund_address: String,
    pub cancel_timelock: CancelTimelock,
    pub punish_timelock: PunishTimelock,
    pub monero_receive_pool: MoneroAddressPool,
}

impl Request for GetSwapInfoArgs {
    type Response = GetSwapInfoResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        get_swap_info(self, ctx).await
    }
}

// GetSwapTimelock
#[typeshare]
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GetSwapTimelockArgs {
    #[typeshare(serialized_as = "string")]
    pub swap_id: Uuid,
}

#[typeshare]
#[derive(Serialize)]
pub struct GetSwapTimelockResponse {
    #[typeshare(serialized_as = "string")]
    pub swap_id: Uuid,
    pub timelock: Option<ExpiredTimelocks>,
}

impl Request for GetSwapTimelockArgs {
    type Response = GetSwapTimelockResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        get_swap_timelock(self, ctx).await
    }
}

// Balance
#[typeshare]
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BalanceArgs {
    pub force_refresh: bool,
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BalanceResponse {
    #[typeshare(serialized_as = "number")]
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    pub balance: bitcoin::Amount,
}

impl Request for BalanceArgs {
    type Response = BalanceResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        get_balance(self, ctx).await
    }
}

// GetBitcoinAddress
#[typeshare]
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GetBitcoinAddressArgs;

#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetBitcoinAddressResponse {
    #[typeshare(serialized_as = "string")]
    #[serde(with = "swap_serde::bitcoin::address_serde")]
    pub address: bitcoin::Address,
}

impl Request for GetBitcoinAddressArgs {
    type Response = GetBitcoinAddressResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        let bitcoin_wallet = ctx.try_get_bitcoin_wallet().await?;
        let address = bitcoin_wallet.new_address().await?;

        Ok(GetBitcoinAddressResponse { address })
    }
}

// GetHistory
#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct GetHistoryArgs;

#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetHistoryEntry {
    #[typeshare(serialized_as = "string")]
    swap_id: Uuid,
    state: String,
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct GetHistoryResponse {
    pub swaps: Vec<GetHistoryEntry>,
}

impl Request for GetHistoryArgs {
    type Response = GetHistoryResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        get_history(ctx).await
    }
}

// Additional structs
#[typeshare]
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct AliceAddress {
    #[typeshare(serialized_as = "string")]
    pub peer_id: PeerId,
    pub addresses: Vec<String>,
}

// Suspend current swap
#[derive(Debug, Deserialize)]
pub struct SuspendCurrentSwapArgs;

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct SuspendCurrentSwapResponse {
    // If no swap was running, we still return Ok(...) but this is set to None
    #[typeshare(serialized_as = "Option<string>")]
    pub swap_id: Option<Uuid>,
}

impl Request for SuspendCurrentSwapArgs {
    type Response = SuspendCurrentSwapResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        suspend_current_swap(ctx).await
    }
}

#[typeshare]
#[derive(Debug, Serialize, Deserialize)]
pub struct GetCurrentSwapArgs;

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct GetCurrentSwapResponse {
    #[typeshare(serialized_as = "Option<string>")]
    pub swap_id: Option<Uuid>,
}

impl Request for GetCurrentSwapArgs {
    type Response = GetCurrentSwapResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        get_current_swap(ctx).await
    }
}

pub struct GetConfig;

impl Request for GetConfig {
    type Response = serde_json::Value;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        get_config(ctx).await
    }
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct ExportBitcoinWalletArgs;

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct ExportBitcoinWalletResponse {
    #[typeshare(serialized_as = "object")]
    pub wallet_descriptor: serde_json::Value,
}

impl Request for ExportBitcoinWalletArgs {
    type Response = ExportBitcoinWalletResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        let wallet_descriptor = export_bitcoin_wallet(ctx).await?;
        Ok(ExportBitcoinWalletResponse { wallet_descriptor })
    }
}

pub struct GetConfigArgs;

impl Request for GetConfigArgs {
    type Response = serde_json::Value;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        get_config(ctx).await
    }
}

pub struct GetSwapInfosAllArgs;

impl Request for GetSwapInfosAllArgs {
    type Response = Vec<GetSwapInfoResponse>;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        get_swap_infos_all(ctx).await
    }
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct GetLogsArgs {
    #[typeshare(serialized_as = "Option<string>")]
    pub swap_id: Option<Uuid>,
    pub redact: bool,
    #[typeshare(serialized_as = "Option<string>")]
    pub logs_dir: Option<PathBuf>,
}

#[typeshare]
#[derive(Serialize, Debug)]
pub struct GetLogsResponse {
    logs: Vec<String>,
}

impl Request for GetLogsArgs {
    type Response = GetLogsResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        let config = ctx.try_get_config().await?;
        let dir = self.logs_dir.unwrap_or(config.log_dir.clone());
        let logs = get_logs(dir, self.swap_id, self.redact).await?;

        for msg in &logs {
            println!("{msg}");
        }

        Ok(GetLogsResponse { logs })
    }
}

/// Best effort redaction of logs, e.g. wallet addresses, swap-ids
#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct RedactArgs {
    pub text: String,
}

#[typeshare]
#[derive(Serialize, Debug)]
pub struct RedactResponse {
    pub text: String,
}

impl Request for RedactArgs {
    type Response = RedactResponse;

    async fn request(self, _: Arc<Context>) -> Result<Self::Response> {
        Ok(RedactResponse {
            text: redact(&self.text),
        })
    }
}

#[typeshare]
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GetRestoreHeightArgs;

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct GetRestoreHeightResponse {
    #[typeshare(serialized_as = "number")]
    pub height: u64,
}

impl Request for GetRestoreHeightArgs {
    type Response = GetRestoreHeightResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        let wallet_manager = ctx.try_get_monero_manager().await?;
        let wallet = wallet_manager.main_wallet().await;
        let height = wallet.get_restore_height().await?;

        Ok(GetRestoreHeightResponse { height })
    }
}

#[typeshare]
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GetMoneroAddressesArgs;

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct GetMoneroAddressesResponse {
    #[typeshare(serialized_as = "Vec<String>")]
    pub addresses: Vec<monero::Address>,
}

impl Request for GetMoneroAddressesArgs {
    type Response = GetMoneroAddressesResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        let db = ctx.try_get_db().await?;
        let addresses = db.get_monero_addresses().await?;
        Ok(GetMoneroAddressesResponse { addresses })
    }
}

#[typeshare]
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GetMoneroHistoryArgs;

#[typeshare]
#[derive(Serialize, Clone, Deserialize, Debug)]
pub struct GetMoneroHistoryResponse {
    pub transactions: Vec<monero_sys::TransactionInfo>,
}

impl Request for GetMoneroHistoryArgs {
    type Response = GetMoneroHistoryResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        let wallet_manager = ctx.try_get_monero_manager().await?;
        let wallet = wallet_manager.main_wallet().await;

        let transactions = wallet.history().await;
        Ok(GetMoneroHistoryResponse { transactions })
    }
}

#[typeshare]
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GetMoneroMainAddressArgs;

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct GetMoneroMainAddressResponse {
    #[typeshare(serialized_as = "String")]
    pub address: monero::Address,
}

impl Request for GetMoneroMainAddressArgs {
    type Response = GetMoneroMainAddressResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        let wallet_manager = ctx.try_get_monero_manager().await?;
        let wallet = wallet_manager.main_wallet().await;
        let address = wallet.main_address().await;
        Ok(GetMoneroMainAddressResponse { address })
    }
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Date {
    #[typeshare(serialized_as = "number")]
    pub year: u16,
    #[typeshare(serialized_as = "number")]
    pub month: u8,
    #[typeshare(serialized_as = "number")]
    pub day: u8,
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", content = "height")]
pub enum SetRestoreHeightArgs {
    #[typeshare(serialized_as = "number")]
    Height(u32),
    #[typeshare(serialized_as = "object")]
    Date(Date),
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct SetRestoreHeightResponse {
    pub success: bool,
}

impl Request for SetRestoreHeightArgs {
    type Response = SetRestoreHeightResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        let wallet_manager = ctx.try_get_monero_manager().await?;
        let wallet = wallet_manager.main_wallet().await;

        let height = match self {
            SetRestoreHeightArgs::Height(height) => height as u64,
            SetRestoreHeightArgs::Date(date) => {
                let year: u16 = date.year;
                let month: u8 = date.month;
                let day: u8 = date.day;
                // Validate ranges
                if month < 1 || month > 12 {
                    bail!("Month must be between 1 and 12");
                }
                if day < 1 || day > 31 {
                    bail!("Day must be between 1 and 31");
                }

                tracing::info!(
                    "Getting blockchain height for date: {}-{}-{}",
                    year,
                    month,
                    day
                );

                let height = wallet
                    .get_blockchain_height_by_date(year, month, day)
                    .await
                    .with_context(|| {
                        format!(
                            "Failed to get blockchain height for date {}-{}-{}",
                            year, month, day
                        )
                    })?;
                tracing::info!(
                    "Blockchain height for date {}-{}-{}: {}",
                    year,
                    month,
                    day,
                    height
                );

                height
            }
        };

        wallet.set_restore_height(height).await?;
        wallet.pause_refresh().await;
        wallet.stop().await;
        tracing::debug!("Background refresh stopped");

        wallet.rescan_blockchain_async().await;
        wallet.start_refresh().await;
        tracing::info!("Rescanning blockchain from height {} completed", height);

        Ok(SetRestoreHeightResponse { success: true })
    }
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct SetMoneroWalletPasswordArgs {
    pub password: String,
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct SetMoneroWalletPasswordResponse {
    pub success: bool,
}

impl Request for SetMoneroWalletPasswordArgs {
    type Response = SetMoneroWalletPasswordResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        let wallet_manager = ctx.try_get_monero_manager().await?;
        let wallet = wallet_manager.main_wallet().await;

        let success = wallet.set_password(self.password).await?;
        if success {
            wallet.store_in_current_file().await?;
        }

        Ok(SetMoneroWalletPasswordResponse { success })
    }
}

// New request type for Monero balance
#[typeshare]
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GetMoneroBalanceArgs;

#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetMoneroBalanceResponse {
    #[typeshare(serialized_as = "string")]
    pub total_balance: crate::monero::Amount,
    #[typeshare(serialized_as = "string")]
    pub unlocked_balance: crate::monero::Amount,
}

impl Request for GetMoneroBalanceArgs {
    type Response = GetMoneroBalanceResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        let wallet_manager = ctx.try_get_monero_manager().await?;
        let wallet = wallet_manager.main_wallet().await;

        let total_balance = wallet.total_balance().await;
        let unlocked_balance = wallet.unlocked_balance().await;

        Ok(GetMoneroBalanceResponse {
            total_balance: crate::monero::Amount::from_piconero(total_balance.as_pico()),
            unlocked_balance: crate::monero::Amount::from_piconero(unlocked_balance.as_pico()),
        })
    }
}

#[typeshare]
#[derive(Debug, Serialize, Deserialize)]
pub struct SendMoneroArgs {
    #[typeshare(serialized_as = "String")]
    pub address: String,
    pub amount: SendMoneroAmount,
}

#[typeshare]
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "amount")]
pub enum SendMoneroAmount {
    Sweep,
    Specific(crate::monero::Amount),
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct SendMoneroResponse {
    pub tx_hash: String,
    pub address: String,
    pub amount_sent: crate::monero::Amount,
    pub fee: crate::monero::Amount,
}

impl Request for SendMoneroArgs {
    type Response = SendMoneroResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        let wallet_manager = ctx.try_get_monero_manager().await?;
        let wallet = wallet_manager.main_wallet().await;

        // Parse the address
        let address = monero::Address::from_str(&self.address)
            .map_err(|e| anyhow::anyhow!("Invalid Monero address: {}", e))?;

        let tauri_handle = ctx
            .tauri_handle
            .clone()
            .context("Tauri needs to be available to approve transactions")?;

        // This is a closure that will be called by the monero-sys library to get approval for the transaction
        // It sends an approval request to the frontend and returns true if the user approves the transaction
        let approval_callback: Arc<
            dyn Fn(String, ::monero::Amount, ::monero::Amount) -> BoxFuture<'static, bool>
                + Send
                + Sync,
        > = std::sync::Arc::new(
            move |_txid: String, amount: ::monero::Amount, fee: ::monero::Amount| {
                let tauri_handle = tauri_handle.clone();

                Box::pin(async move {
                    let details = SendMoneroDetails {
                        address: address.to_string(),
                        amount: amount.into(),
                        fee: fee.into(),
                    };

                    tauri_handle
                        .request_approval::<bool>(
                            ApprovalRequestType::SendMonero(details),
                            Some(60 * 5),
                        )
                        .await
                        .unwrap_or(false)
                })
            },
        );

        let amount = match self.amount {
            SendMoneroAmount::Sweep => None,
            SendMoneroAmount::Specific(amount) => Some(amount.into()),
        };

        // This is the actual call to the monero-sys library to send the transaction
        // monero-sys will call the approval callback after it has constructed and signed the transaction
        // once the user approves, the transaction is published
        let (receipt, amount_sent, fee) = wallet
            .transfer_with_approval(&address, amount, approval_callback)
            .await?
            .context("Transaction was not approved by user")?;

        Ok(SendMoneroResponse {
            tx_hash: receipt.txid,
            address: address.to_string(),
            amount_sent: amount_sent.into(),
            fee: fee.into(),
        })
    }
}

#[tracing::instrument(fields(method = "suspend_current_swap"), skip(context))]
pub async fn suspend_current_swap(context: Arc<Context>) -> Result<SuspendCurrentSwapResponse> {
    let swap_id = context.swap_lock.get_current_swap_id().await;

    if let Some(id_value) = swap_id {
        context.swap_lock.send_suspend_signal().await?;

        Ok(SuspendCurrentSwapResponse {
            swap_id: Some(id_value),
        })
    } else {
        // If no swap was running, we still return Ok(...) with None
        Ok(SuspendCurrentSwapResponse { swap_id: None })
    }
}

#[tracing::instrument(fields(method = "get_swap_infos_all"), skip(context))]
pub async fn get_swap_infos_all(context: Arc<Context>) -> Result<Vec<GetSwapInfoResponse>> {
    let db = context.try_get_db().await?;
    let swap_ids = db.all().await?;
    let mut swap_infos = Vec::new();

    for (swap_id, _) in swap_ids {
        match get_swap_info(GetSwapInfoArgs { swap_id }, context.clone()).await {
            Ok(swap_info) => swap_infos.push(swap_info),
            Err(error) => {
                tracing::error!(%swap_id, %error, "Failed to get swap info");
            }
        }
    }

    Ok(swap_infos)
}

#[tracing::instrument(fields(method = "get_swap_info"), skip(context))]
pub async fn get_swap_info(
    args: GetSwapInfoArgs,
    context: Arc<Context>,
) -> Result<GetSwapInfoResponse> {
    let db = context.try_get_db().await?;

    let state = db.get_state(args.swap_id).await?;
    let is_completed = state.swap_finished();

    let peer_id = db
        .get_peer_id(args.swap_id)
        .await
        .with_context(|| "Could not get PeerID")?;

    let addresses = db
        .get_addresses(peer_id)
        .await
        .with_context(|| "Could not get addressess")?;

    let start_date = db.get_swap_start_date(args.swap_id).await?;

    let swap_state: BobState = state.try_into()?;

    let (
        xmr_amount,
        btc_amount,
        tx_lock_id,
        tx_cancel_fee,
        tx_refund_fee,
        tx_lock_fee,
        btc_refund_address,
        cancel_timelock,
        punish_timelock,
    ) = db
        .get_states(args.swap_id)
        .await?
        .iter()
        .find_map(|state| {
            let State::Bob(BobState::SwapSetupCompleted(state2)) = state else {
                return None;
            };

            let xmr_amount = state2.xmr;
            let btc_amount = state2.tx_lock.lock_amount();
            let tx_cancel_fee = state2.tx_cancel_fee;
            let tx_refund_fee = state2.tx_refund_fee;
            let tx_lock_id = state2.tx_lock.txid();
            let btc_refund_address = state2.refund_address.to_string();

            let Ok(tx_lock_fee) = state2.tx_lock.fee() else {
                return None;
            };

            Some((
                xmr_amount,
                btc_amount,
                tx_lock_id,
                tx_cancel_fee,
                tx_refund_fee,
                tx_lock_fee,
                btc_refund_address,
                state2.cancel_timelock,
                state2.punish_timelock,
            ))
        })
        .with_context(|| "Did not find SwapSetupCompleted state for swap")?;

    let monero_receive_pool = db.get_monero_address_pool(args.swap_id).await?;

    Ok(GetSwapInfoResponse {
        swap_id: args.swap_id,
        seller: AliceAddress {
            peer_id,
            addresses: addresses.iter().map(|a| a.to_string()).collect(),
        },
        completed: is_completed,
        start_date,
        state_name: format!("{}", swap_state),
        xmr_amount,
        btc_amount,
        tx_lock_id,
        tx_cancel_fee,
        tx_refund_fee,
        tx_lock_fee,
        btc_refund_address: btc_refund_address.to_string(),
        cancel_timelock,
        punish_timelock,
        monero_receive_pool,
    })
}

#[tracing::instrument(fields(method = "get_swap_timelock"), skip(context))]
pub async fn get_swap_timelock(
    args: GetSwapTimelockArgs,
    context: Arc<Context>,
) -> Result<GetSwapTimelockResponse> {
    let bitcoin_wallet = context.try_get_bitcoin_wallet().await?;
    let db = context.try_get_db().await?;

    let state = db.get_state(args.swap_id).await?;
    let swap_state: BobState = state.try_into()?;

    let timelock = swap_state.expired_timelocks(bitcoin_wallet.clone()).await?;

    Ok(GetSwapTimelockResponse {
        swap_id: args.swap_id,
        timelock,
    })
}

#[tracing::instrument(fields(method = "buy_xmr"), skip(context))]
pub async fn buy_xmr(
    buy_xmr: BuyXmrArgs,
    swap_id: Uuid,
    context: Arc<Context>,
) -> Result<(), anyhow::Error> {
    let _span = get_swap_tracing_span(swap_id);

    let BuyXmrArgs {
        sellers,
        bitcoin_change_address,
        monero_receive_pool,
    } = buy_xmr;

    let config = context.try_get_config().await?;
    let db = context.try_get_db().await?;

    monero_receive_pool.assert_network(config.env_config.monero_network)?;
    monero_receive_pool.assert_sum_to_one()?;

    let bitcoin_wallet = context.try_get_bitcoin_wallet().await?;

    let bitcoin_change_address = match bitcoin_change_address {
        Some(addr) => addr
            .require_network(bitcoin_wallet.network())
            .context("Address is not on the correct network")?,
        None => {
            let internal_wallet_address = bitcoin_wallet.new_address().await?;

            tracing::info!(
                internal_wallet_address=%internal_wallet_address,
                "No --change-address supplied. Any change will be received to the internal wallet."
            );

            internal_wallet_address
        }
    };

    let monero_wallet = context.try_get_monero_manager().await?;

    let env_config = config.env_config;

    // Prepare variables for the quote fetching process
    let tauri_handle = context.tauri_handle.clone();

    // Get the existing event loop handle from context
    let mut event_loop_handle = context.try_get_event_loop_handle().await?;
    let quotes_rx = event_loop_handle.cached_quotes();

    // Wait for the user to approve a seller and to deposit coins
    // Calling determine_btc_to_swap
    let address_len = bitcoin_wallet.new_address().await?.script_pubkey().len();

    let bitcoin_wallet_for_closures = Arc::clone(&bitcoin_wallet);

    // Clone variables before moving them into closures
    let bitcoin_change_address_for_spawn = bitcoin_change_address.clone();

    // Clone tauri_handle for different closures
    let tauri_handle_for_determine = tauri_handle.clone();
    let tauri_handle_for_selection = tauri_handle.clone();
    let tauri_handle_for_suspension = tauri_handle.clone();

    // Acquire the lock before the user has selected a maker and we already have funds in the wallet
    // because we need to be able to cancel the determine_btc_to_swap(..)
    context.swap_lock.acquire_swap_lock(swap_id).await?;

    let select_offer_result = tokio::select! {
        result = determine_btc_to_swap(
            quotes_rx,
            bitcoin_wallet.new_address(),
            {
                let wallet = Arc::clone(&bitcoin_wallet_for_closures);
                move || {
                    let w = wallet.clone();
                    async move { w.balance().await }
                }
            },
            {
                let wallet = Arc::clone(&bitcoin_wallet_for_closures);
                move || {
                    let w = wallet.clone();
                    async move { w.max_giveable(address_len).await }
                }
            },
            {
                let wallet = Arc::clone(&bitcoin_wallet_for_closures);
                move || {
                    let w = wallet.clone();
                    async move { w.sync().await }
                }
            },
            tauri_handle_for_determine,
            swap_id,
            |quote_with_address| {
                let tauri_handle_clone = tauri_handle_for_selection.clone();
                Box::new(async move {
                    let details = SelectMakerDetails {
                        swap_id,
                        btc_amount_to_swap: quote_with_address.quote.max_quantity,
                        maker: quote_with_address,
                    };

                    tauri_handle_clone.request_maker_selection(details, 300).await
                }) as Box<dyn Future<Output = Result<bool>> + Send>
            },
        ) => {
            Some(result?)
        }
        _ = context.swap_lock.listen_for_swap_force_suspension() => {
            context.swap_lock.release_swap_lock().await.expect("Shutdown signal received but failed to release swap lock. The swap process has been terminated but the swap lock is still active.");

            if let Some(handle) = tauri_handle_for_suspension {
                handle.emit_swap_progress_event(swap_id, TauriSwapProgressEvent::Released);
            }

            None
        },
    };

    let Some((seller_multiaddr, seller_peer_id, quote, tx_lock_amount, tx_lock_fee)) =
        select_offer_result
    else {
        return Ok(());
    };

    // Insert the peer_id into the database
    db.insert_peer_id(swap_id, seller_peer_id).await?;

    db.insert_address(seller_peer_id, seller_multiaddr.clone())
        .await?;

    db.insert_monero_address_pool(swap_id, monero_receive_pool.clone())
        .await?;

    // Add the seller's address to the swarm
    event_loop_handle
        .queue_peer_address(seller_peer_id, seller_multiaddr.clone())
        .await?;

    tauri_handle.emit_swap_progress_event(
        swap_id,
        TauriSwapProgressEvent::ReceivedQuote(quote.clone()),
    );

    tauri_handle.emit_swap_progress_event(swap_id, TauriSwapProgressEvent::ReceivedQuote(quote));

    context.tasks.clone().spawn(async move {
        tokio::select! {
            biased;
            _ = context.swap_lock.listen_for_swap_force_suspension() => {
                tracing::debug!("Shutdown signal received, exiting");
                context.swap_lock.release_swap_lock().await.expect("Shutdown signal received but failed to release swap lock. The swap process has been terminated but the swap lock is still active.");

                tauri_handle.emit_swap_progress_event(swap_id, TauriSwapProgressEvent::Released);

                bail!("Shutdown signal received");
            },

            swap_result = async {
                let swap_event_loop_handle = event_loop_handle.swap_handle(seller_peer_id, swap_id).await?;
                let swap = Swap::new(
                    db.clone(),
                    swap_id,
                    bitcoin_wallet.clone(),
                    monero_wallet,
                    env_config,
                    swap_event_loop_handle,
                    monero_receive_pool.clone(),
                    bitcoin_change_address_for_spawn,
                    tx_lock_amount,
                    tx_lock_fee
                ).with_event_emitter(tauri_handle.clone());

                bob::run(swap).await
            } => {
                match swap_result {
                    Ok(state) => {
                        tracing::debug!(%swap_id, state=%state, "Swap completed")
                    }
                    Err(error) => {
                        tracing::error!(%swap_id, "Failed to complete swap: {:#}", error)
                    }
                }
            },
        };
        tracing::debug!(%swap_id, "Swap completed");

        context
            .swap_lock
            .release_swap_lock()
            .await
            .expect("Could not release swap lock");

        tauri_handle.emit_swap_progress_event(swap_id, TauriSwapProgressEvent::Released);

        Ok::<_, anyhow::Error>(())
    }.in_current_span()).await;

    Ok(())
}

#[tracing::instrument(fields(method = "resume_swap"), skip(context))]
pub async fn resume_swap(
    resume: ResumeSwapArgs,
    context: Arc<Context>,
) -> Result<ResumeSwapResponse> {
    let ResumeSwapArgs { swap_id } = resume;

    let db = context.try_get_db().await?;
    let config = context.try_get_config().await?;
    let bitcoin_wallet = context.try_get_bitcoin_wallet().await?;
    let monero_manager = context.try_get_monero_manager().await?;

    let seller_peer_id = db.get_peer_id(swap_id).await?;
    let seller_addresses = db.get_addresses(seller_peer_id).await?;

    let mut event_loop_handle = context.try_get_event_loop_handle().await?;

    for seller_address in seller_addresses {
        event_loop_handle
            .queue_peer_address(seller_peer_id, seller_address)
            .await?;
    }

    let monero_receive_pool = db.get_monero_address_pool(swap_id).await?;

    let tauri_handle = context.tauri_handle.clone();

    let swap_event_loop_handle = event_loop_handle
        .swap_handle(seller_peer_id, swap_id)
        .await?;
    let swap = Swap::from_db(
        db.clone(),
        swap_id,
        bitcoin_wallet,
        monero_manager,
        config.env_config,
        swap_event_loop_handle,
        monero_receive_pool,
    )
    .await?
    .with_event_emitter(tauri_handle.clone());

    context.swap_lock.acquire_swap_lock(swap_id).await?;

    tauri_handle.emit_swap_progress_event(swap_id, TauriSwapProgressEvent::Resuming);

    context.tasks.clone().spawn(
        async move {
            tokio::select! {
                biased;
                _ = context.swap_lock.listen_for_swap_force_suspension() => {
                     tracing::debug!("Shutdown signal received, exiting");
                    context.swap_lock.release_swap_lock().await.expect("Shutdown signal received but failed to release swap lock. The swap process has been terminated but the swap lock is still active.");

                    tauri_handle.emit_swap_progress_event(swap_id, TauriSwapProgressEvent::Released);

                    bail!("Shutdown signal received");
                },

                swap_result = bob::run(swap) => {
                    match swap_result {
                        Ok(state) => {
                            tracing::debug!(%swap_id, state=%state, "Swap completed after resuming")
                        }
                        Err(error) => {
                            tracing::error!(%swap_id, "Failed to resume swap: {:#}", error)
                        }
                    }

                }
            }
            context
                .swap_lock
                .release_swap_lock()
                .await
                .expect("Could not release swap lock");

            tauri_handle.emit_swap_progress_event(swap_id, TauriSwapProgressEvent::Released);

            Ok::<(), anyhow::Error>(())
        }
        .in_current_span(),
    ).await;

    Ok(ResumeSwapResponse {
        result: "OK".to_string(),
    })
}

#[tracing::instrument(fields(method = "cancel_and_refund"), skip(context))]
pub async fn cancel_and_refund(
    cancel_and_refund: CancelAndRefundArgs,
    context: Arc<Context>,
) -> Result<serde_json::Value> {
    let CancelAndRefundArgs { swap_id } = cancel_and_refund;
    let bitcoin_wallet = context.try_get_bitcoin_wallet().await?;
    let db = context.try_get_db().await?;

    context.swap_lock.acquire_swap_lock(swap_id).await?;

    let state = cli::cancel_and_refund(swap_id, bitcoin_wallet, db).await;

    context
        .swap_lock
        .release_swap_lock()
        .await
        .expect("Could not release swap lock");

    context
        .tauri_handle
        .emit_swap_progress_event(swap_id, TauriSwapProgressEvent::Released);

    state.map(|state| {
        json!({
            "result": state,
        })
    })
}

#[tracing::instrument(fields(method = "get_history"), skip(context))]
pub async fn get_history(context: Arc<Context>) -> Result<GetHistoryResponse> {
    let db = context.try_get_db().await?;
    let swaps = db.all().await?;
    let mut vec: Vec<GetHistoryEntry> = Vec::new();
    for (swap_id, state) in swaps {
        let state: BobState = state.try_into()?;
        vec.push(GetHistoryEntry {
            swap_id,
            state: state.to_string(),
        })
    }

    Ok(GetHistoryResponse { swaps: vec })
}

#[tracing::instrument(fields(method = "get_config"), skip(context))]
pub async fn get_config(context: Arc<Context>) -> Result<serde_json::Value> {
    let config = context.try_get_config().await?;
    let data_dir_display = config.data_dir.display();
    tracing::info!(path=%data_dir_display, "Data directory");
    tracing::info!(path=%format!("{}/logs", data_dir_display), "Log files directory");
    tracing::info!(path=%format!("{}/sqlite", data_dir_display), "Sqlite file location");
    tracing::info!(path=%format!("{}/seed.pem", data_dir_display), "Seed file location");
    tracing::info!(path=%format!("{}/monero", data_dir_display), "Monero-wallet-rpc directory");
    tracing::info!(path=%format!("{}/wallet", data_dir_display), "Internal bitcoin wallet directory");

    Ok(json!({
        "log_files": format!("{}/logs", data_dir_display),
        "sqlite": format!("{}/sqlite", data_dir_display),
        "seed": format!("{}/seed.pem", data_dir_display),
        "monero-wallet-rpc": format!("{}/monero", data_dir_display),
        "bitcoin_wallet": format!("{}/wallet", data_dir_display),
    }))
}

#[tracing::instrument(fields(method = "withdraw_btc"), skip(context))]
pub async fn withdraw_btc(
    withdraw_btc: WithdrawBtcArgs,
    context: Arc<Context>,
) -> Result<WithdrawBtcResponse> {
    let WithdrawBtcArgs { address, amount } = withdraw_btc;
    let bitcoin_wallet = context.try_get_bitcoin_wallet().await?;

    let (withdraw_tx_unsigned, amount) = match amount {
        Some(amount) => {
            let withdraw_tx_unsigned = bitcoin_wallet
                .send_to_address_dynamic_fee(address, amount, None)
                .await?;

            (withdraw_tx_unsigned, amount)
        }
        None => {
            let (max_giveable, spending_fee) = bitcoin_wallet
                .max_giveable(address.script_pubkey().len())
                .await?;

            let withdraw_tx_unsigned = bitcoin_wallet
                .send_to_address(address, max_giveable, spending_fee, None)
                .await?;

            (withdraw_tx_unsigned, max_giveable)
        }
    };

    let withdraw_tx = bitcoin_wallet
        .sign_and_finalize(withdraw_tx_unsigned)
        .await?;

    bitcoin_wallet
        .broadcast(withdraw_tx.clone(), "withdraw")
        .await?;

    let txid = withdraw_tx.compute_txid();

    Ok(WithdrawBtcResponse {
        txid: txid.to_string(),
        amount,
    })
}

#[tracing::instrument(fields(method = "get_balance"), skip(context))]
pub async fn get_balance(balance: BalanceArgs, context: Arc<Context>) -> Result<BalanceResponse> {
    let BalanceArgs { force_refresh } = balance;
    let bitcoin_wallet = context.try_get_bitcoin_wallet().await?;

    if force_refresh {
        bitcoin_wallet.sync().await?;
    }

    let bitcoin_balance = bitcoin_wallet.balance().await?;

    if force_refresh {
        tracing::info!(
            balance = %bitcoin_balance,
            "Checked Bitcoin balance",
        );
    } else {
        tracing::debug!(
            balance = %bitcoin_balance,
            "Current Bitcoin balance as of last sync",
        );
    }

    Ok(BalanceResponse {
        balance: bitcoin_balance,
    })
}

#[tracing::instrument(fields(method = "export_bitcoin_wallet"), skip(context))]
pub async fn export_bitcoin_wallet(context: Arc<Context>) -> Result<serde_json::Value> {
    let bitcoin_wallet = context.try_get_bitcoin_wallet().await?;

    let wallet_export = bitcoin_wallet.wallet_export("cli").await?;
    tracing::info!(descriptor=%wallet_export.to_string(), "Exported bitcoin wallet");
    Ok(json!({
        "descriptor": wallet_export.to_string(),
    }))
}

#[tracing::instrument(fields(method = "monero_recovery"), skip(context))]
pub async fn monero_recovery(
    monero_recovery: MoneroRecoveryArgs,
    context: Arc<Context>,
) -> Result<serde_json::Value> {
    let MoneroRecoveryArgs { swap_id } = monero_recovery;
    let db = context.try_get_db().await?;
    let config = context.try_get_config().await?;

    let swap_state: BobState = db.get_state(swap_id).await?.try_into()?;

    if let BobState::BtcRedeemed(state5) = swap_state {
        let (spend_key, view_key) = state5.xmr_keys();
        let restore_height = state5.monero_wallet_restore_blockheight.height;

        let address = monero::Address::standard(
            config.env_config.monero_network,
            monero::PublicKey::from_private_key(&spend_key),
            monero::PublicKey::from(view_key.public()),
        );

        tracing::info!(restore_height=%restore_height, address=%address, spend_key=%spend_key, view_key=%view_key, "Monero recovery information");

        Ok(json!({
            "address": address,
            "spend_key": spend_key.to_string(),
            "view_key": view_key.to_string(),
            "restore_height": state5.monero_wallet_restore_blockheight.height,
        }))
    } else {
        bail!(
            "Cannot print monero recovery information in state {}, only possible for BtcRedeemed",
            swap_state
        )
    }
}

#[tracing::instrument(fields(method = "get_current_swap"), skip(context))]
pub async fn get_current_swap(context: Arc<Context>) -> Result<GetCurrentSwapResponse> {
    let swap_id = context.swap_lock.get_current_swap_id().await;
    Ok(GetCurrentSwapResponse { swap_id })
}

// TODO: Let this take a refresh interval as an argument
pub async fn refresh_wallet_task<FMG, TMG, FB, TB, FS, TS>(
    max_giveable_fn: FMG,
    balance_fn: FB,
    sync_fn: FS,
) -> Result<(
    tokio::task::JoinHandle<()>,
    ::tokio::sync::watch::Receiver<(bitcoin::Amount, bitcoin::Amount)>,
)>
where
    TMG: Future<Output = Result<(bitcoin::Amount, bitcoin::Amount)>> + Send + 'static,
    FMG: Fn() -> TMG + Send + 'static,
    TB: Future<Output = Result<bitcoin::Amount>> + Send + 'static,
    FB: Fn() -> TB + Send + 'static,
    TS: Future<Output = Result<()>> + Send + 'static,
    FS: Fn() -> TS + Send + 'static,
{
    let (tx, rx) = ::tokio::sync::watch::channel((bitcoin::Amount::ZERO, bitcoin::Amount::ZERO));

    let handle = tokio::task::spawn(async move {
        loop {
            // Sync wallet before checking balance
            let _ = sync_fn().await;

            if let (Ok(balance), Ok((max_giveable, _fee))) =
                (balance_fn().await, max_giveable_fn().await)
            {
                let _ = tx.send((balance, max_giveable));
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    });

    Ok((handle, rx))
}

pub type QuoteFetchFuture = BoxFuture<
    'static,
    Result<(
        tokio::task::JoinHandle<()>,
        ::tokio::sync::watch::Receiver<Vec<SellerStatus>>,
    )>,
>;

#[allow(clippy::too_many_arguments)]
pub async fn determine_btc_to_swap<FB, TB, FMG, TMG, FS, TS>(
    mut quotes_rx: ::tokio::sync::watch::Receiver<
        Vec<(PeerId, libp2p::Multiaddr, BidQuote, Option<semver::Version>)>,
    >,
    // TODO: Shouldn't this be a function?
    get_new_address: impl Future<Output = Result<bitcoin::Address>>,
    balance: FB,
    max_giveable_fn: FMG,
    sync: FS,
    event_emitter: Option<TauriHandle>,
    swap_id: Uuid,
    request_approval: impl Fn(QuoteWithAddress) -> Box<dyn Future<Output = Result<bool>> + Send>,
) -> Result<(
    Multiaddr,
    PeerId,
    BidQuote,
    bitcoin::Amount,
    bitcoin::Amount,
)>
where
    TB: Future<Output = Result<bitcoin::Amount>> + Send + 'static,
    FB: Fn() -> TB + Send + 'static,
    TMG: Future<Output = Result<(bitcoin::Amount, bitcoin::Amount)>> + Send + 'static,
    FMG: Fn() -> TMG + Send + 'static,
    TS: Future<Output = Result<()>> + Send + 'static,
    FS: Fn() -> TS + Send + 'static,
{
    // Start background tasks with watch channels
    let (wallet_refresh_handle, mut balance_rx): (
        _,
        ::tokio::sync::watch::Receiver<(bitcoin::Amount, bitcoin::Amount)>,
    ) = refresh_wallet_task(max_giveable_fn, balance, sync).await?;

    // Get the abort handles to kill the background tasks when we exit the function
    let wallet_refresh_abort_handle = AbortOnDropHandle::new(wallet_refresh_handle);

    let mut pending_approvals = FuturesUnordered::new();

    let deposit_address = get_new_address.await?;

    loop {
        // Get the latest quotes, balance and max_giveable
        let quotes = quotes_rx.borrow().clone();
        let quotes: Vec<SellerStatus> = quotes
            .into_iter()
            .map(|(peer_id, multiaddr, quote, version)| {
                SellerStatus::Online(QuoteWithAddress {
                    multiaddr,
                    peer_id,
                    quote,
                    version,
                })
            })
            .collect();
        let (balance, max_giveable) = *balance_rx.borrow();

        let success_quotes = quotes
            .iter()
            .filter_map(|quote| match quote {
                SellerStatus::Online(quote_with_address) => Some(quote_with_address.clone()),
                SellerStatus::Unreachable(_) => None,
            })
            .collect::<Vec<_>>();

        // Emit a Tauri event
        event_emitter.emit_swap_progress_event(
            swap_id,
            TauriSwapProgressEvent::WaitingForBtcDeposit {
                deposit_address: deposit_address.clone(),
                max_giveable: max_giveable,
                min_bitcoin_lock_tx_fee: balance - max_giveable,
                known_quotes: success_quotes.clone(),
            },
        );

        // Iterate through quotes and find ones that match the balance and max_giveable
        let matching_quotes = success_quotes
            .iter()
            .filter_map(|quote_with_address| {
                let quote = quote_with_address.quote;

                if quote.min_quantity <= max_giveable && quote.max_quantity > bitcoin::Amount::ZERO
                {
                    let tx_lock_fee = balance - max_giveable;
                    let tx_lock_amount = std::cmp::min(max_giveable, quote.max_quantity);

                    Some((quote_with_address.clone(), tx_lock_amount, tx_lock_fee))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        // Put approval requests into FuturesUnordered
        for (quote, tx_lock_amount, tx_lock_fee) in matching_quotes {
            let future = request_approval(quote.clone());

            pending_approvals.push(async move {
                use std::pin::Pin;
                let pinned_future = Pin::from(future);
                let approved = pinned_future.await?;

                if approved {
                    Ok::<
                        Option<(
                            Multiaddr,
                            PeerId,
                            BidQuote,
                            bitcoin::Amount,
                            bitcoin::Amount,
                        )>,
                        anyhow::Error,
                    >(Some((
                        quote.multiaddr.clone(),
                        quote.peer_id.clone(),
                        quote.quote.clone(),
                        tx_lock_amount,
                        tx_lock_fee,
                    )))
                } else {
                    Ok::<
                        Option<(
                            Multiaddr,
                            PeerId,
                            BidQuote,
                            bitcoin::Amount,
                            bitcoin::Amount,
                        )>,
                        anyhow::Error,
                    >(None)
                }
            });
        }

        // Listen for approvals, balance changes, or quote changes
        let result: Option<(
            Multiaddr,
            PeerId,
            BidQuote,
            bitcoin::Amount,
            bitcoin::Amount,
        )> = tokio::select! {
            // Any approval request completes
            approval_result = pending_approvals.next(), if !pending_approvals.is_empty() => {
                match approval_result {
                    Some(Ok(Some(result))) => Some(result),
                    Some(Ok(None)) => None, // User rejected
                    Some(Err(_)) => None,   // Error in approval
                    None => None,           // No more futures
                }
            }
            // Balance changed - drop all pending approval requests and and re-calculate
            _ = balance_rx.changed() => {
                pending_approvals.clear();
                None
            }
            // Quotes changed - drop all pending approval requests and re-calculate
            _ = quotes_rx.changed() => {
                pending_approvals.clear();
                None
            }
        };

        // If user accepted an offer, return it to start the swap
        if let Some((multiaddr, peer_id, quote, tx_lock_amount, tx_lock_fee)) = result {
            wallet_refresh_abort_handle.abort();

            return Ok((multiaddr, peer_id, quote, tx_lock_amount, tx_lock_fee));
        }

        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

#[typeshare]
#[derive(Deserialize, Serialize)]
pub struct CheckMoneroNodeArgs {
    pub url: String,
    pub network: String,
}

#[typeshare]
#[derive(Deserialize, Serialize)]
pub struct CheckMoneroNodeResponse {
    pub available: bool,
}

#[typeshare]
#[derive(Deserialize, Serialize)]
pub struct GetDataDirArgs {
    pub is_testnet: bool,
}

#[derive(Error, Debug)]
#[error("this is not one of the known monero networks")]
struct UnknownMoneroNetwork(String);

impl CheckMoneroNodeArgs {
    pub async fn request(self) -> Result<CheckMoneroNodeResponse> {
        let url = self.url.clone();
        let network_str = self.network.clone();

        let network = match self.network.to_lowercase().as_str() {
            // When the GUI says testnet, it means monero stagenet
            "mainnet" => Network::Mainnet,
            "testnet" => Network::Stagenet,
            otherwise => anyhow::bail!(UnknownMoneroNetwork(otherwise.to_string())),
        };

        static CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
            reqwest::Client::builder()
                // This function is called very frequently, so we set the timeout to be short
                .timeout(Duration::from_secs(5))
                .https_only(false)
                .build()
                .expect("reqwest client to work")
        });

        let Ok(monero_daemon) = MoneroDaemon::from_str(self.url, network) else {
            return Ok(CheckMoneroNodeResponse { available: false });
        };

        match monero_daemon.is_available(&CLIENT).await {
            Ok(available) => Ok(CheckMoneroNodeResponse { available }),
            Err(e) => {
                tracing::error!(
                    url = %url,
                    network = %network_str,
                    error = ?e,
                    error_chain = %format!("{:#}", e),
                    "Failed to check monero node availability"
                );

                Ok(CheckMoneroNodeResponse { available: false })
            }
        }
    }
}

#[typeshare]
#[derive(Deserialize, Clone)]
pub struct CheckElectrumNodeArgs {
    pub url: String,
}

#[typeshare]
#[derive(Serialize, Clone)]
pub struct CheckElectrumNodeResponse {
    pub available: bool,
}

impl CheckElectrumNodeArgs {
    pub async fn request(self) -> Result<CheckElectrumNodeResponse> {
        // Check if the URL is valid
        let Ok(url) = Url::parse(&self.url) else {
            return Ok(CheckElectrumNodeResponse { available: false });
        };

        // Check if the node is available
        let res = wallet::Client::new(&[url.as_str().to_string()], Duration::from_secs(60)).await;

        Ok(CheckElectrumNodeResponse {
            available: res.is_ok(),
        })
    }
}

#[typeshare]
#[derive(Debug, Serialize, Deserialize)]
pub struct ResolveApprovalArgs {
    pub request_id: String,
    #[typeshare(serialized_as = "object")]
    pub accept: serde_json::Value,
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct ResolveApprovalResponse {
    pub success: bool,
}

#[typeshare]
#[derive(Debug, Serialize, Deserialize)]
pub struct RejectApprovalArgs {
    pub request_id: String,
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct RejectApprovalResponse {
    pub success: bool,
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct CheckSeedArgs {
    pub seed: String,
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct CheckSeedResponse {
    pub available: bool,
}

impl CheckSeedArgs {
    pub async fn request(self) -> Result<CheckSeedResponse> {
        let seed = MoneroSeed::from_string(Language::English, Zeroizing::new(self.seed));
        Ok(CheckSeedResponse {
            available: seed.is_ok(),
        })
    }
}

// New request type for Monero sync progress
#[typeshare]
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GetMoneroSyncProgressArgs;

#[typeshare]
#[derive(Serialize, Clone, Deserialize, Debug)]
pub struct GetMoneroSyncProgressResponse {
    #[typeshare(serialized_as = "number")]
    pub current_block: u64,
    #[typeshare(serialized_as = "number")]
    pub target_block: u64,
    #[typeshare(serialized_as = "number")]
    pub progress_percentage: f32,
}

impl Request for GetMoneroSyncProgressArgs {
    type Response = GetMoneroSyncProgressResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        let wallet_manager = ctx.try_get_monero_manager().await?;
        let wallet = wallet_manager.main_wallet().await;

        let sync_progress = wallet.call(|wallet| wallet.sync_progress()).await;

        Ok(GetMoneroSyncProgressResponse {
            current_block: sync_progress.current_block,
            target_block: sync_progress.target_block,
            progress_percentage: sync_progress.percentage(),
        })
    }
}

#[typeshare]
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GetMoneroSeedArgs;

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct GetMoneroSeedResponse {
    pub seed: String,
}

impl Request for GetMoneroSeedArgs {
    type Response = GetMoneroSeedResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        let wallet_manager = ctx.try_get_monero_manager().await?;
        let wallet = wallet_manager.main_wallet().await;

        let seed = wallet.seed().await?;

        Ok(GetMoneroSeedResponse { seed })
    }
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct GetPendingApprovalsResponse {
    pub approvals: Vec<crate::cli::api::tauri_bindings::ApprovalRequest>,
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct DfxAuthenticateResponse {
    pub access_token: String,
    pub kyc_url: String,
}

// ChangeMoneroNode
#[typeshare]
#[derive(Debug, Serialize, Deserialize)]
pub struct ChangeMoneroNodeArgs {
    pub node_config: MoneroNodeConfig,
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug)]
pub struct ChangeMoneroNodeResponse {
    pub success: bool,
}

impl Request for ChangeMoneroNodeArgs {
    type Response = ChangeMoneroNodeResponse;

    async fn request(self, ctx: Arc<Context>) -> Result<Self::Response> {
        change_monero_node(self, ctx).await
    }
}

#[tracing::instrument(fields(method = "change_monero_node"), skip(context))]
pub async fn change_monero_node(
    args: ChangeMoneroNodeArgs,
    context: Arc<Context>,
) -> Result<ChangeMoneroNodeResponse> {
    context.change_monero_node(args.node_config).await?;

    Ok(ChangeMoneroNodeResponse { success: true })
}
