use crate::out_event;
use crate::protocols::swap_setup;
use crate::protocols::swap_setup::verification::{ProofVerifier, VerificationError};
use crate::protocols::swap_setup::{
    BlockchainNetwork, SpotPriceError, SpotPriceRequest, SpotPriceResponse, protocol,
};
use anyhow::{Context, Result, anyhow};
use futures::AsyncWriteExt;
use futures::FutureExt;
use futures::StreamExt;
use futures::future::BoxFuture;
use futures::stream::FuturesUnordered;
use libp2p::core::upgrade;
use libp2p::swarm::handler::ConnectionEvent;
use libp2p::swarm::{ConnectionHandler, ConnectionId};
use libp2p::swarm::{ConnectionHandlerEvent, NetworkBehaviour, SubstreamProtocol, ToSwarm};
use libp2p::{Multiaddr, PeerId};
use std::collections::VecDeque;
use std::fmt::Debug;
use std::task::Poll;
use std::time::Duration;
use swap_core::bitcoin;
use swap_env::env;
use swap_feed::LatestRate;
use swap_machine::alice::{State0, State3};
use swap_machine::common::{Message0, Message2, Message4};
use tracing::Instrument;
use uuid::Uuid;

const MAX_TX_LOCK_INPUTS: usize = 25;

#[derive(Debug)]
pub enum LockInputCheckError {
    Cancelled,
    TimedOut,
    Invalid(String),
    Unavailable(String),
}

impl From<bitcoin_wallet::PsbtPrevoutVerificationError> for LockInputCheckError {
    fn from(error: bitcoin_wallet::PsbtPrevoutVerificationError) -> Self {
        if matches!(
            error,
            bitcoin_wallet::PsbtPrevoutVerificationError::Cancelled
        ) {
            return Self::Cancelled;
        }
        if matches!(
            error,
            bitcoin_wallet::PsbtPrevoutVerificationError::TimedOut
        ) {
            return Self::TimedOut;
        }
        let is_transient = error.is_transient();
        let reason = format!("{error:#}");
        if is_transient {
            Self::Unavailable(reason)
        } else {
            Self::Invalid(reason)
        }
    }
}

/// Local request; cancellation stops new RPC work, and the response confirms
/// that already-started verification work has finished.
#[derive(Debug)]
pub struct LockInputCheck {
    pub psbt: bitcoin::PartiallySignedTransaction,
    pub cancel: tokio::sync::watch::Receiver<bool>,
}

/// Keeps the inputs of Bob's lock transaction reserved for one swap setup
/// until dropped.
#[derive(Debug)]
pub struct LockInputReservation {
    outpoints: Vec<::bitcoin::OutPoint>,
    release: tokio::sync::mpsc::UnboundedSender<Vec<::bitcoin::OutPoint>>,
}

impl LockInputReservation {
    pub fn new(
        outpoints: Vec<::bitcoin::OutPoint>,
        release: tokio::sync::mpsc::UnboundedSender<Vec<::bitcoin::OutPoint>>,
    ) -> Self {
        Self { outpoints, release }
    }
}

impl Drop for LockInputReservation {
    fn drop(&mut self) {
        // The receiver is only gone once the event loop holding the reservations is gone
        let _ = self.release.send(std::mem::take(&mut self.outpoints));
    }
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum OutEvent {
    Initiated {
        send_wallet_snapshot:
            bmrng::RequestReceiver<bitcoin::Amount, (WalletSnapshot, bitcoin::Amount, bool)>,
        check_lock_inputs: bmrng::RequestReceiver<
            LockInputCheck,
            Result<LockInputReservation, LockInputCheckError>,
        >,
    },
    Completed {
        peer_id: PeerId,
        swap_id: Uuid,
        state3: State3,
        lock_input_reservation: LockInputReservation,
    },
    Error {
        peer_id: PeerId,
        error: anyhow::Error,
    },
}

#[derive(Debug)]
pub struct WalletSnapshot {
    unlocked_balance: swap_core::monero::Amount,
    lock_fee: swap_core::monero::Amount,

    // TODO: Consider using the same address for punish and redeem (they are mutually exclusive, so
    // effectively the address will only be used once)
    redeem_address: bitcoin::Address,
    punish_address: bitcoin::Address,

    // Local estimates; setup derives the agreed fees once both output scripts are known.
    redeem_fee: bitcoin::Amount,
    cancel_fee: bitcoin::Amount,
    refund_fee: bitcoin::Amount,
    partial_refund_fee: bitcoin::Amount,
    reclaim_fee: bitcoin::Amount,
    mercy_fee: bitcoin::Amount,
    punish_fee: bitcoin::Amount,
    withhold_fee: bitcoin::Amount,
    fee_rate: bitcoin::SwapFeeRate,
    fee_limits: bitcoin::SwapFeeLimits,
}

impl WalletSnapshot {
    pub fn new(
        unlocked_balance: swap_core::monero::Amount,
        redeem_address: bitcoin::Address,
        punish_address: bitcoin::Address,
        redeem_fee: bitcoin::Amount,
        cancel_fee: bitcoin::Amount,
        refund_fee: bitcoin::Amount,
        partial_refund_fee: bitcoin::Amount,
        reclaim_fee: bitcoin::Amount,
        mercy_fee: bitcoin::Amount,
        punish_fee: bitcoin::Amount,
        withhold_fee: bitcoin::Amount,
        fee_rate: bitcoin::SwapFeeRate,
        fee_limits: bitcoin::SwapFeeLimits,
    ) -> Self {
        Self {
            unlocked_balance,
            lock_fee: swap_core::monero::CONSERVATIVE_MONERO_FEE,
            redeem_address,
            punish_address,
            redeem_fee,
            cancel_fee,
            refund_fee,
            partial_refund_fee,
            reclaim_fee,
            mercy_fee,
            punish_fee,
            withhold_fee,
            fee_rate,
            fee_limits,
        }
    }
}

impl From<OutEvent> for out_event::alice::OutEvent {
    fn from(event: OutEvent) -> Self {
        match event {
            OutEvent::Initiated {
                send_wallet_snapshot,
                check_lock_inputs,
            } => out_event::alice::OutEvent::SwapSetupInitiated {
                send_wallet_snapshot,
                check_lock_inputs,
            },
            OutEvent::Completed {
                peer_id: bob_peer_id,
                swap_id,
                state3,
                lock_input_reservation,
            } => out_event::alice::OutEvent::SwapSetupCompleted {
                peer_id: bob_peer_id,
                swap_id,
                state3,
                lock_input_reservation,
            },
            OutEvent::Error { peer_id, error } => out_event::alice::OutEvent::Failure {
                peer: peer_id,
                error: anyhow!(error),
            },
        }
    }
}

#[allow(missing_debug_implementations)]
pub struct Behaviour<LR> {
    events: VecDeque<OutEvent>,
    min_buy: bitcoin::Amount,
    max_buy: bitcoin::Amount,
    env_config: env::Config,

    latest_rate: LR,
    resume_only: bool,
    proof_verifier: ProofVerifier,
}

impl<LR> Behaviour<LR> {
    pub fn new(
        min_buy: bitcoin::Amount,
        max_buy: bitcoin::Amount,
        env_config: env::Config,
        latest_rate: LR,
        resume_only: bool,
    ) -> Self {
        Self {
            events: Default::default(),
            min_buy,
            max_buy,
            env_config,
            latest_rate,
            resume_only,
            proof_verifier: ProofVerifier::spawn(),
        }
    }
}

impl<LR> NetworkBehaviour for Behaviour<LR>
where
    LR: LatestRate + Send + 'static + Clone,
{
    type ConnectionHandler = Handler<LR>;
    type ToSwarm = OutEvent;

    fn handle_established_inbound_connection(
        &mut self,
        connection_id: libp2p::swarm::ConnectionId,
        peer: PeerId,
        _local_addr: &Multiaddr,
        _remote_addr: &Multiaddr,
    ) -> std::result::Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        // A new inbound connection has been established by Bob
        // He wants to negotiate a swap setup with us
        // We create a new Handler to handle the negotiation
        let handler = Handler::new(
            peer,
            connection_id,
            self.min_buy,
            self.max_buy,
            self.env_config,
            self.latest_rate.clone(),
            self.resume_only,
            self.proof_verifier.clone(),
        );

        Ok(handler)
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: libp2p::swarm::ConnectionId,
        peer: PeerId,
        _addr: &Multiaddr,
        _role_override: libp2p::core::Endpoint,
        _port_use: libp2p::core::transport::PortUse,
    ) -> std::result::Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        // A new outbound connection has been established (probably to a rendezvous node because we dont dial Bob)
        // We still return a handler, because we dont want to close the connection
        let handler = Handler::new(
            peer,
            connection_id,
            self.min_buy,
            self.max_buy,
            self.env_config,
            self.latest_rate.clone(),
            self.resume_only,
            self.proof_verifier.clone(),
        );

        Ok(handler)
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        _: ConnectionId,
        event: HandlerOutEvent,
    ) {
        // Here we receive events from the Handler, add some context and forward them to the swarm
        // This is done by pushing the event to the [`events`] queue
        // The queue is then polled in the [`poll`] function, and the events are sent to the swarm
        match event {
            HandlerOutEvent::Initiated {
                send_wallet_snapshot,
                check_lock_inputs,
            } => self.events.push_back(OutEvent::Initiated {
                send_wallet_snapshot,
                check_lock_inputs,
            }),
            HandlerOutEvent::Completed(Ok((swap_id, state3, lock_input_reservation))) => {
                self.events.push_back(OutEvent::Completed {
                    peer_id,
                    swap_id,
                    state3,
                    lock_input_reservation,
                })
            }
            HandlerOutEvent::Completed(Err(error)) => {
                self.events.push_back(OutEvent::Error { peer_id, error })
            }
        }
    }

    fn poll(&mut self, _cx: &mut std::task::Context<'_>) -> Poll<ToSwarm<Self::ToSwarm, ()>> {
        // Poll events from the queue and send them to the swarm
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(ToSwarm::GenerateEvent(event));
        }

        Poll::Pending
    }

    fn on_swarm_event(&mut self, _event: libp2p::swarm::FromSwarm<'_>) {
        // We do not need to handle any swarm events here
    }
}

pub struct Handler<LR> {
    inbound_streams:
        FuturesUnordered<BoxFuture<'static, Result<(Uuid, State3, LockInputReservation)>>>,
    events: VecDeque<HandlerOutEvent>,

    peer_id: PeerId,
    connection_id: ConnectionId,

    min_buy: bitcoin::Amount,
    max_buy: bitcoin::Amount,
    env_config: env::Config,

    latest_rate: LR,
    resume_only: bool,

    proof_verifier: ProofVerifier,

    // This is the timeout for the negotiation phase where Alice and Bob exchange messages
    negotiation_timeout: Duration,
}

impl<LR> Handler<LR> {
    fn new(
        peer_id: PeerId,
        connection_id: ConnectionId,
        min_buy: bitcoin::Amount,
        max_buy: bitcoin::Amount,
        env_config: env::Config,
        latest_rate: LR,
        resume_only: bool,
        proof_verifier: ProofVerifier,
    ) -> Self {
        Self {
            inbound_streams: FuturesUnordered::new(),
            events: Default::default(),
            peer_id,
            connection_id,
            min_buy,
            max_buy,
            env_config,
            latest_rate,
            resume_only,
            proof_verifier,
            negotiation_timeout: crate::defaults::NEGOTIATION_TIMEOUT,
        }
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum HandlerOutEvent {
    Initiated {
        send_wallet_snapshot:
            bmrng::RequestReceiver<bitcoin::Amount, (WalletSnapshot, bitcoin::Amount, bool)>,
        check_lock_inputs: bmrng::RequestReceiver<
            LockInputCheck,
            Result<LockInputReservation, LockInputCheckError>,
        >,
    },
    Completed(Result<(Uuid, State3, LockInputReservation)>),
}

impl<LR> ConnectionHandler for Handler<LR>
where
    LR: LatestRate + Send + 'static,
{
    type FromBehaviour = ();
    type ToBehaviour = HandlerOutEvent;
    type InboundProtocol = protocol::SwapSetup;
    type OutboundProtocol = upgrade::DeniedUpgrade;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(protocol::new(), ())
    }

    fn on_connection_event(
        &mut self,
        event: libp2p::swarm::handler::ConnectionEvent<
            '_,
            Self::InboundProtocol,
            Self::OutboundProtocol,
            Self::InboundOpenInfo,
            Self::OutboundOpenInfo,
        >,
    ) {
        match event {
            ConnectionEvent::FullyNegotiatedInbound(substream) => {
                let substream = substream.protocol;

                let (sender, receiver) =
                    bmrng::channel_with_timeout::<
                        bitcoin::Amount,
                        (WalletSnapshot, bitcoin::Amount, bool),
                    >(1, crate::defaults::SWAP_SETUP_CHANNEL_TIMEOUT);

                let (check_lock_inputs_sender, check_lock_inputs_receiver) = bmrng::channel::<
                    LockInputCheck,
                    Result<LockInputReservation, LockInputCheckError>,
                >(1);

                let resume_only = self.resume_only;
                let min_buy = self.min_buy;
                let max_buy = self.max_buy;
                let latest_rate = self.latest_rate.latest_rate();
                let env_config = self.env_config;

                // We wrap the entire handshake in a timeout future
                let protocol = tokio::time::timeout(
                    self.negotiation_timeout,
                    run_swap_setup(
                        substream,
                        sender,
                        check_lock_inputs_sender,
                        self.proof_verifier.clone(),
                        self.peer_id,
                        resume_only,
                        env_config,
                        min_buy,
                        max_buy,
                        latest_rate.map_err(|error| {
                            Box::new(error) as Box<dyn std::error::Error + Send + Sync + 'static>
                        }),
                    ),
                );

                // Attach a span so every log emitted during the negotiation is
                // attributable to the peer and connection it belongs to.
                let span = tracing::info_span!(
                    "swap_setup",
                    peer = %self.peer_id,
                    connection = %self.connection_id,
                );

                let max_seconds = self.negotiation_timeout.as_secs();
                self.inbound_streams.push(
                    async move {
                        tracing::debug!("Inbound swap setup negotiation started");

                        let result = match protocol.await {
                            Ok(result) => result,
                            Err(_elapsed) => {
                                tracing::warn!(
                                    timeout_seconds = max_seconds,
                                    "Swap setup timed out"
                                );
                                return Err(anyhow!(
                                    "Failed to complete execution setup within {}s",
                                    max_seconds
                                ));
                            }
                        };

                        match &result {
                            Ok((swap_id, _, _)) => {
                                tracing::info!(%swap_id, "Swap setup completed")
                            }
                            Err(error) => {
                                tracing::warn!(error = ?error, "Swap setup failed")
                            }
                        }

                        result
                    }
                    .instrument(span)
                    .boxed(),
                );

                self.events.push_back(HandlerOutEvent::Initiated {
                    send_wallet_snapshot: receiver,
                    check_lock_inputs: check_lock_inputs_receiver,
                });
            }
            ConnectionEvent::DialUpgradeError(..) => {
                unreachable!("Alice does not dial")
            }
            ConnectionEvent::FullyNegotiatedOutbound(..) => {
                unreachable!("Alice does not support outbound connections")
            }
            _ => {}
        }
    }

    fn on_behaviour_event(&mut self, _event: Self::FromBehaviour) {
        unreachable!("Alice does not receive events from the Behaviour in the handler")
    }

    fn connection_keep_alive(&self) -> bool {
        !self.inbound_streams.is_empty()
    }

    #[allow(clippy::type_complexity)]
    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::ToBehaviour>,
    > {
        // Send events in the queue to the behaviour
        // This is currently only used to notify the behaviour that the negotiation phase has been initiated
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(event));
        }

        if let Poll::Ready(Some(result)) = self.inbound_streams.poll_next_unpin(cx) {
            // Notify the behaviour that the negotiation phase has been completed
            // (either successfully or with an error)
            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                HandlerOutEvent::Completed(result),
            ));
        }

        Poll::Pending
    }
}

impl SpotPriceResponse {
    pub fn from_result_ref(result: &Result<swap_core::monero::Amount, Error>) -> Self {
        match result {
            Ok(amount) => SpotPriceResponse::Xmr(*amount),
            Err(error) => SpotPriceResponse::Error(error.to_error_response()),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("ASB is running in resume-only mode")]
    ResumeOnlyMode,
    #[error("Amount {buy} below minimum {min}")]
    AmountBelowMinimum {
        min: bitcoin::Amount,
        buy: bitcoin::Amount,
    },
    #[error("Amount {buy} above maximum {max}")]
    AmountAboveMaximum {
        max: bitcoin::Amount,
        buy: bitcoin::Amount,
    },
    #[error("Unlocked balance ({balance}) too low to fulfill swapping {buy}")]
    BalanceTooLow {
        balance: swap_core::monero::Amount,
        buy: bitcoin::Amount,
    },
    #[error("Failed to fetch latest rate")]
    LatestRateFetchFailed(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),
    #[error("Failed to calculate quote")]
    SellQuoteCalculationFailed(#[source] anyhow::Error),
    #[error("Blockchain networks did not match, we are on {asb:?}, but request from {cli:?}")]
    BlockchainNetworkMismatch {
        cli: BlockchainNetwork,
        asb: BlockchainNetwork,
    },
}

impl Error {
    pub fn to_error_response(&self) -> SpotPriceError {
        match self {
            Error::ResumeOnlyMode => SpotPriceError::NoSwapsAccepted,
            Error::AmountBelowMinimum { min, buy } => SpotPriceError::AmountBelowMinimum {
                min: *min,
                buy: *buy,
            },
            Error::AmountAboveMaximum { max, buy } => SpotPriceError::AmountAboveMaximum {
                max: *max,
                buy: *buy,
            },
            Error::BalanceTooLow { buy, .. } => SpotPriceError::BalanceTooLow { buy: *buy },
            Error::BlockchainNetworkMismatch { cli, asb } => {
                SpotPriceError::BlockchainNetworkMismatch {
                    cli: *cli,
                    asb: *asb,
                }
            }
            Error::LatestRateFetchFailed(_) | Error::SellQuoteCalculationFailed(_) => {
                SpotPriceError::Other
            }
        }
    }
}

async fn run_swap_setup(
    mut substream: libp2p::swarm::Stream,
    sender: bmrng::RequestSender<bitcoin::Amount, (WalletSnapshot, bitcoin::Amount, bool)>,
    check_lock_inputs: bmrng::RequestSender<
        LockInputCheck,
        Result<LockInputReservation, LockInputCheckError>,
    >,
    proof_verifier: ProofVerifier,
    peer_id: PeerId,
    resume_only: bool,
    env_config: env::Config,
    min_buy: bitcoin::Amount,
    max_buy: bitcoin::Amount,
    latest_rate: Result<swap_feed::Rate, Box<dyn std::error::Error + Send + Sync + 'static>>,
) -> Result<(Uuid, State3, LockInputReservation)> {
    let request = swap_setup::read_cbor_message::<SpotPriceRequest>(&mut substream)
        .await
        .context("Failed to read spot price request")?
        .context("Peer sent an error instead of spot price request")?;

    let (wallet_snapshot, btc_amnesty_amount, should_burn_on_refund) = sender
        .send_receive(request.btc)
        .await
        .context("Failed to receive wallet snapshot")?;

    // wrap all of these into another future so we can `return` from all the
    // different blocks
    let validate = async {
        if resume_only {
            return Err(Error::ResumeOnlyMode);
        };

        let blockchain_network = BlockchainNetwork {
            bitcoin: env_config.bitcoin_network,
            monero: env_config.monero_network,
        };

        if request.blockchain_network != blockchain_network {
            return Err(Error::BlockchainNetworkMismatch {
                cli: request.blockchain_network,
                asb: blockchain_network,
            });
        }

        let btc = request.btc;

        if btc < min_buy {
            return Err(Error::AmountBelowMinimum {
                min: min_buy,
                buy: btc,
            });
        }

        if btc > max_buy {
            return Err(Error::AmountAboveMaximum {
                max: max_buy,
                buy: btc,
            });
        }

        let rate = latest_rate.map_err(Error::LatestRateFetchFailed)?;
        let xmr = rate
            .sell_quote(btc)
            .map_err(Error::SellQuoteCalculationFailed)?;

        let unlocked = wallet_snapshot.unlocked_balance;

        let needed_balance = xmr + wallet_snapshot.lock_fee.into();
        if unlocked.as_pico() < needed_balance.as_pico() {
            tracing::warn!(
                unlocked_balance = %unlocked,
                needed_balance = %needed_balance,
                "Rejecting swap, unlocked balance too low"
            );
            return Err(Error::BalanceTooLow {
                balance: wallet_snapshot.unlocked_balance,
                buy: btc,
            });
        }

        Ok(xmr)
    };

    let result = validate.await;

    let converted_result = match result {
        Ok(xmr) => Ok(xmr.into()),
        Err(e) => Err(e),
    };
    swap_setup::write_cbor_message(
        &mut substream,
        SpotPriceResponse::from_result_ref(&converted_result),
    )
    .await
    .context("Failed to write spot price response")?;

    let xmr = converted_result?;

    tracing::debug!(
        redeem_fee = %wallet_snapshot.redeem_fee,
        cancel_fee = %wallet_snapshot.cancel_fee,
        refund_fee = %wallet_snapshot.refund_fee,
        partial_refund_fee = %wallet_snapshot.partial_refund_fee,
        reclaim_fee = %wallet_snapshot.reclaim_fee,
        mercy_fee = %wallet_snapshot.mercy_fee,
        punish_fee = %wallet_snapshot.punish_fee,
        withhold_fee = %wallet_snapshot.withhold_fee,
        "Local Bitcoin transaction fee estimates before setup"
    );

    let state0 = State0::new(
        request.btc,
        xmr,
        btc_amnesty_amount,
        env_config,
        wallet_snapshot.redeem_address,
        wallet_snapshot.punish_address,
        wallet_snapshot.fee_rate,
        wallet_snapshot.fee_limits,
        should_burn_on_refund,
        &mut rand::thread_rng(),
    );

    let message0 = swap_setup::read_cbor_message::<Message0>(&mut substream)
        .await
        .context("Failed to read message0")?
        .context("Peer sent an error instead of message0")?;

    let (swap_id, state1) = match state0.receive(message0) {
        Ok(state) => state,
        Err(error) => {
            swap_setup::write_cbor_error(
                &mut substream,
                swap_setup::SwapSetupError::InvalidFeeProposal(format!("{error:#}")),
            )
            .await?;
            return Err(error).context("Failed to accept swap setup");
        }
    };

    tracing::debug!(%swap_id, "Swap setup transition: State0 -> State1 (received Message0)");

    swap_setup::write_cbor_message(
        &mut substream,
        state1
            .next_message()
            .context("Couldn't construct Mesage1")?,
    )
    .await
    .context("Failed to send message1")?;

    let message2 = swap_setup::read_cbor_message::<Message2>(&mut substream)
        .await
        .context("Failed to read message2")?
        .context("Peer sent an error instead of message2")?;
    let tx_lock_input_count = message2.tx_lock_psbt.unsigned_tx.input.len();
    if tx_lock_input_count > MAX_TX_LOCK_INPUTS {
        let error = swap_setup::SwapSetupError::TxLockTooManyInputs {
            actual: u32::try_from(tx_lock_input_count).context("Input count exceeds wire range")?,
            maximum: u32::try_from(MAX_TX_LOCK_INPUTS).context("Input limit exceeds wire range")?,
        };
        if let Err(write_error) = swap_setup::write_cbor_error(&mut substream, error.clone()).await
        {
            tracing::error!(
                error = %write_error,
                "Couldn't send error message to Bob after encountering it, closing connection"
            );
        }
        return Err(error.into());
    }

    let (state2, lock_input_reservation) = match proof_verifier
        .verify_lock(peer_id, state1, message2, check_lock_inputs)
        .await
    {
        Ok(verified) => verified,
        Err(VerificationError::LockInputs(LockInputCheckError::Invalid(reason))) => {
            let error = swap_setup::SwapSetupError::LockInputVerificationFailed { reason };
            if let Err(err) = swap_setup::write_cbor_error(&mut substream, error.clone()).await {
                tracing::error!(error=%err, "Couldn't send lock input verification error to Bob");
            }
            return Err(error).context("Lock transaction input verification failed");
        }
        Err(error) => return Err(error).context("Lock transaction verification failed"),
    };

    tracing::debug!(%swap_id, "Swap setup transition: State1 -> State2 (received Message2)");

    swap_setup::write_cbor_message(
        &mut substream,
        state2.next_message().context("Couldn't produce Message3")?,
    )
    .await
    .context("Failed to send message3")?;

    let message4 = swap_setup::read_cbor_message::<Message4>(&mut substream)
        .await
        .context("Failed to read message4")?
        .context("Peer sent an error instead of message4")?;
    let state3 = state2
        .receive(message4)
        .context("Failed to transition state2 -> state3 using message4")?;

    tracing::debug!(%swap_id, "Swap setup transition: State2 -> State3 (received Message4)");

    substream
        .flush()
        .await
        .context("Failed to flush substream after all messages were sent")?;
    substream
        .close()
        .await
        .context("Failed to close substream after all messages were sent")?;

    Ok((swap_id, state3, lock_input_reservation))
}
