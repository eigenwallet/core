use crate::asb::event_loop::EventLoopService;
use crate::monero;
use crate::protocol::Database;
use anyhow::{Context, Result};
use bitcoin_wallet::BitcoinWallet;
use jsonrpsee::server::{ServerBuilder, ServerHandle};
use jsonrpsee::types::ErrorObjectOwned;
use jsonrpsee::types::error::ErrorCode;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::{Decimal, RoundingStrategy};
use std::sync::Arc;
use swap_controller_api::{
    ActiveConnectionsResponse, AsbApiServer, BitcoinBalanceResponse, BitcoinSeedResponse,
    MoneroAddressResponse, MoneroBalanceResponse, MoneroSeedResponse, MultiaddressesResponse,
    PeerIdResponse, RegistrationStatusItem, RegistrationStatusResponse, RendezvousConnectionStatus,
    RendezvousRegistrationStatus, Swap, WithdrawBtcResponse, WormholeServiceItem,
    WormholeServicesResponse,
};
use swap_core::monero::PICONERO_OFFSET;
use tokio_util::task::AbortOnDropHandle;
use uuid::Uuid;

pub struct RpcServer {
    handle: ServerHandle,
}

impl RpcServer {
    pub async fn start(
        host: String,
        port: u16,
        bitcoin_wallet: Arc<dyn BitcoinWallet>,
        monero_wallet: Arc<monero::Wallets>,
        event_loop_service: EventLoopService,
        db: Arc<dyn Database + Send + Sync>,
    ) -> Result<Self> {
        let server = ServerBuilder::default()
            .build((host, port))
            .await
            .context("Failed to build RPC server")?;

        let addr = server.local_addr()?;

        let rpc_impl = RpcImpl {
            bitcoin_wallet,
            monero_wallet,
            event_loop_service,
            db,
        };
        let handle = server.start(rpc_impl.into_rpc());

        tracing::info!("JSON-RPC server listening on {}", addr);

        Ok(Self { handle })
    }

    /// Spawn the server in a new tokio task
    pub fn spawn(self) -> AbortOnDropHandle<()> {
        AbortOnDropHandle::new(tokio::spawn(async move {
            self.handle.stopped().await;
        }))
    }
}

pub struct RpcImpl {
    bitcoin_wallet: Arc<dyn BitcoinWallet>,
    monero_wallet: Arc<monero::Wallets>,
    event_loop_service: EventLoopService,
    db: Arc<dyn Database + Send + Sync>,
}

#[async_trait::async_trait]
impl AsbApiServer for RpcImpl {
    async fn check_connection(&self) -> Result<(), ErrorObjectOwned> {
        Ok(())
    }

    async fn bitcoin_balance(&self) -> Result<BitcoinBalanceResponse, ErrorObjectOwned> {
        let balance = self.bitcoin_wallet.balance().await.into_json_rpc_result()?;

        Ok(BitcoinBalanceResponse { balance })
    }

    async fn bitcoin_seed(&self) -> Result<BitcoinSeedResponse, ErrorObjectOwned> {
        static EXPORT_ROLE: &str = "asb";

        let wallet_export = self
            .bitcoin_wallet
            .wallet_export(EXPORT_ROLE)
            .await
            .into_json_rpc_result()?;

        Ok(BitcoinSeedResponse {
            descriptor: format!("{}", wallet_export.descriptor()),
        })
    }

    async fn monero_balance(&self) -> Result<MoneroBalanceResponse, ErrorObjectOwned> {
        let wallet = self.monero_wallet.main_wallet().await;
        let balance = wallet.total_balance().await.into_json_rpc_result()?;

        Ok(MoneroBalanceResponse {
            balance: balance.as_pico(),
        })
    }

    async fn monero_address(&self) -> Result<MoneroAddressResponse, ErrorObjectOwned> {
        let wallet = self.monero_wallet.main_wallet().await;
        let address = wallet.main_address().await.into_json_rpc_result()?;

        Ok(MoneroAddressResponse {
            address: address.to_string(),
        })
    }

    async fn monero_seed(&self) -> Result<MoneroSeedResponse, ErrorObjectOwned> {
        let wallet = self.monero_wallet.main_wallet().await;
        let seed = wallet.seed().await.into_json_rpc_result()?;
        let restore_height = wallet.get_restore_height().await.into_json_rpc_result()?;

        Ok(MoneroSeedResponse {
            seed,
            restore_height,
        })
    }

    async fn multiaddresses(&self) -> Result<MultiaddressesResponse, ErrorObjectOwned> {
        let (_, addresses) = self
            .event_loop_service
            .get_multiaddresses()
            .await
            .into_json_rpc_result()?;

        // TODO: Concenate peer id to the multiaddresses
        let multiaddresses = addresses.iter().map(|addr| addr.to_string()).collect();

        Ok(MultiaddressesResponse { multiaddresses })
    }

    async fn peer_id(&self) -> Result<PeerIdResponse, ErrorObjectOwned> {
        let (peer_id, _) = self
            .event_loop_service
            .get_multiaddresses()
            .await
            .into_json_rpc_result()?;

        Ok(PeerIdResponse {
            peer_id: peer_id.to_string(),
        })
    }

    async fn active_connections(&self) -> Result<ActiveConnectionsResponse, ErrorObjectOwned> {
        let connections = self
            .event_loop_service
            .get_active_connections()
            .await
            .into_json_rpc_result()?;

        Ok(ActiveConnectionsResponse { connections })
    }

    async fn get_swaps(&self) -> Result<Vec<Swap>, ErrorObjectOwned> {
        use crate::protocol::State;
        use crate::protocol::alice::{AliceState, is_complete};

        let swaps = self
            .db
            .all()
            .await
            .context("Error fetching all swap's from database")
            .into_json_rpc_result()?;
        let mut results = Vec::with_capacity(swaps.len());

        for (swap_id, _) in swaps {
            let (current, starting) = self
                .db
                .get_current_and_starting_state(swap_id)
                .await
                .context("Error fetching current and first state from database")
                .into_json_rpc_result()?;

            let (current_alice, state3) = match (current, starting) {
                (
                    State::Alice(current_alice),
                    State::Alice(AliceState::BtcLockTransactionSeen { state3 }),
                ) => (current_alice, state3),
                (
                    State::Alice(AliceState::SafelyAborted),
                    State::Alice(AliceState::SafelyAborted),
                ) => {
                    continue;
                }
                (State::Alice(current_alice), State::Alice(starting_alice)) => {
                    tracing::error!(
                        %swap_id,
                        current_state = %current_alice,
                        starting_state = %starting_alice,
                        "Skipping swap with unexpected state history in get_swaps"
                    );
                    continue;
                }
                _ => continue, // Skip non-Alice swaps
            };

            let start_date = self
                .db
                .get_swap_start_date(swap_id)
                .await
                .into_json_rpc_result()?;
            let peer_id = self.db.get_peer_id(swap_id).await.into_json_rpc_result()?;

            let exchange_rate =
                calculate_exchange_rate(state3.btc, state3.xmr).into_json_rpc_result()?;

            results.push(Swap {
                swap_id: swap_id.to_string(),
                start_date,
                state: current_alice.to_string(),
                btc_lock_txid: state3.tx_lock.txid().to_string(),
                btc_amount: state3.btc,
                xmr_amount: state3.xmr.as_pico(),
                exchange_rate,
                peer_id: peer_id.to_string(),
                completed: is_complete(&current_alice),
            });
        }

        Ok(results)
    }

    async fn registration_status(&self) -> Result<RegistrationStatusResponse, ErrorObjectOwned> {
        let regs = self
            .event_loop_service
            .get_registration_status()
            .await
            .into_json_rpc_result()?;

        let registrations = regs
            .into_iter()
            .map(|r| RegistrationStatusItem {
                address: r.address.map(|a| a.to_string()),
                connection: if r.is_connected {
                    RendezvousConnectionStatus::Connected
                } else {
                    RendezvousConnectionStatus::Disconnected
                },
                registration: match r.registration {
                    crate::network::rendezvous::register::public::RegistrationStatus::RegisterOnceConnected => {
                        RendezvousRegistrationStatus::RegisterOnceConnected
                    }
                    crate::network::rendezvous::register::public::RegistrationStatus::WillRegisterAfterDelay => {
                        RendezvousRegistrationStatus::WillRegisterAfterDelay
                    }
                    crate::network::rendezvous::register::public::RegistrationStatus::RequestInflight => {
                        RendezvousRegistrationStatus::RequestInflight
                    }
                    crate::network::rendezvous::register::public::RegistrationStatus::Registered => {
                        RendezvousRegistrationStatus::Registered
                    }
                },
            })
            .collect();

        Ok(RegistrationStatusResponse { registrations })
    }

    async fn set_withhold_deposit(
        &self,
        swap_id: Uuid,
        burn: bool,
    ) -> Result<(), ErrorObjectOwned> {
        self.event_loop_service
            .set_withhold_deposit(swap_id, burn)
            .await
            .into_json_rpc_result()?;

        Ok(())
    }

    async fn grant_mercy(&self, swap_id: Uuid) -> Result<(), ErrorObjectOwned> {
        self.event_loop_service
            .grant_mercy(swap_id)
            .await
            .into_json_rpc_result()?;
        Ok(())
    }

    async fn wormhole_services(&self) -> Result<WormholeServicesResponse, ErrorObjectOwned> {
        let services = self
            .event_loop_service
            .get_wormhole_services()
            .await
            .into_json_rpc_result()?;

        let services = services
            .into_iter()
            .map(|info| WormholeServiceItem {
                peer_id: info.peer_id.to_string(),
                address: info.address.to_string(),
                status: info.status,
            })
            .collect();

        Ok(WormholeServicesResponse { services })
    }

    async fn withdraw_btc(
        &self,
        address: String,
        amount: Option<u64>,
    ) -> Result<WithdrawBtcResponse, ErrorObjectOwned> {
        let network = self.bitcoin_wallet.network();
        let address =
            bitcoin_wallet::bitcoin_address::parse_and_validate_network(&address, network)
                .into_json_rpc_result()?;
        let amount = amount.map(bitcoin::Amount::from_sat);

        let (txid, amount) =
            bitcoin_wallet::withdraw(self.bitcoin_wallet.as_ref(), address, amount)
                .await
                .into_json_rpc_result()?;

        Ok(WithdrawBtcResponse {
            amount,
            txid: txid.to_string(),
        })
    }

    async fn refresh_bitcoin_wallet(&self) -> Result<(), ErrorObjectOwned> {
        self.bitcoin_wallet.sync().await.into_json_rpc_result()?;
        Ok(())
    }
}

fn calculate_exchange_rate(btc: bitcoin::Amount, xmr: monero::Amount) -> Result<bitcoin::Amount> {
    let sats_per_xmr = Decimal::from(btc.to_sat())
        .checked_mul(Decimal::from(PICONERO_OFFSET))
        .context("exchange rate overflow")?
        .checked_div(Decimal::from(xmr.as_pico()))
        .context("xmr amount must be greater than zero")?;

    let sats_per_xmr = sats_per_xmr
        .round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero)
        .to_u64()
        .context("exchange rate should fit into satoshis")?;

    Ok(bitcoin::Amount::from_sat(sats_per_xmr))
}

trait IntoJsonRpcResult<T> {
    fn into_json_rpc_result(self) -> Result<T, ErrorObjectOwned>;
}

impl<T> IntoJsonRpcResult<T> for anyhow::Result<T> {
    fn into_json_rpc_result(self) -> Result<T, ErrorObjectOwned> {
        self.map_err(|e| e.into_json_rpc_error())
    }
}

trait IntoJsonRpcError {
    fn into_json_rpc_error(self) -> ErrorObjectOwned;
}

impl IntoJsonRpcError for anyhow::Error {
    fn into_json_rpc_error(self) -> ErrorObjectOwned {
        ErrorObjectOwned::owned(
            ErrorCode::InternalError.code(),
            format!("{self:?}"),
            None::<()>,
        )
    }
}
