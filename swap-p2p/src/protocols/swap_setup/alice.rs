use crate::out_event;
use crate::protocols::swap_setup;
use crate::protocols::swap_setup::verification::{LockInputVerifier, VerificationError};
use crate::protocols::swap_setup::{
    BlockchainNetwork, BobDecision, SpotPriceError, SpotPriceRequest, SpotPriceResponse,
    SwapPersisted, protocol,
};
use anyhow::{Context, Result, anyhow};
use futures::FutureExt;
use futures::SinkExt;
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
use swap_core::bitcoin::{self, MAX_TX_LOCK_INPUTS};
use swap_env::env;
use swap_feed::LatestRate;
use swap_machine::alice::{State0, State3};
use swap_machine::common::{Message0, Message2, Message4};
use tracing::Instrument;
use uuid::Uuid;

/// Deadline for a lock input verification job, counted from when it is queued.
/// A job still queued at the deadline is answered as timed out without
/// starting; a running job stops retrying Electrum at the deadline.
pub const LOCK_INPUT_VERIFICATION_TIMEOUT: Duration = Duration::from_secs(45);

/// The lock input check failed, but may pass later: the inputs are used by
/// another swap that may still be aborted, or we failed to check them.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct LockInputCheckError(pub String);

/// Local request to reserve the inputs of Bob's lock transaction once the
/// [`LockInputVerifier`] verified them. The ASB rejects inputs used by another swap.
#[derive(Debug)]
pub struct LockInputCheck {
    pub psbt: bitcoin::PartiallySignedTransaction,
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

/// Sent once Bob committed to the swap. The ASB persists and starts the swap;
/// only then is Bob told that he may publish the lock transaction.
#[derive(Debug)]
pub struct RegisterSwap {
    pub peer_id: PeerId,
    pub swap_id: Uuid,
    pub state3: State3,
    pub lock_input_reservation: LockInputReservation,
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum OutEvent {
    Initiated {
        /// Span of this swap setup. Work it requests from the event loop is logged under it.
        span: tracing::Span,
        send_wallet_snapshot:
            bmrng::RequestReceiver<bitcoin::Amount, (WalletSnapshot, bitcoin::Amount, bool)>,
        check_lock_inputs: bmrng::RequestReceiver<
            LockInputCheck,
            Result<LockInputReservation, LockInputCheckError>,
        >,
        register_swap: bmrng::RequestReceiver<RegisterSwap, Result<()>>,
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

    /// Local estimate including the relay minimum; sets the lock fee floor.
    estimated_fee_rate: ::bitcoin::FeeRate,
    fee_rate: bitcoin::SwapFeeRate,
    redeem_multiplier: bitcoin::RedeemFeeMultiplier,
    fee_limits: bitcoin::SwapFeeLimits,
}

impl WalletSnapshot {
    pub fn new(
        unlocked_balance: swap_core::monero::Amount,
        redeem_address: bitcoin::Address,
        punish_address: bitcoin::Address,
        estimated_fee_rate: ::bitcoin::FeeRate,
        fee_rate: bitcoin::SwapFeeRate,
        redeem_multiplier: bitcoin::RedeemFeeMultiplier,
        fee_limits: bitcoin::SwapFeeLimits,
    ) -> Self {
        Self {
            unlocked_balance,
            lock_fee: swap_core::monero::CONSERVATIVE_MONERO_FEE,
            redeem_address,
            punish_address,
            estimated_fee_rate,
            fee_rate,
            redeem_multiplier,
            fee_limits,
        }
    }
}

impl From<OutEvent> for out_event::alice::OutEvent {
    fn from(event: OutEvent) -> Self {
        match event {
            OutEvent::Initiated {
                span,
                send_wallet_snapshot,
                check_lock_inputs,
                register_swap,
            } => out_event::alice::OutEvent::SwapSetupInitiated {
                span,
                send_wallet_snapshot,
                check_lock_inputs,
                register_swap,
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
    lock_input_verifier: LockInputVerifier,
}

impl<LR> Behaviour<LR> {
    pub fn register_verification_metrics(
        &self,
        registry: &mut prometheus_client::registry::Registry,
    ) {
        self.lock_input_verifier.register_metrics(registry);
    }

    pub fn new(
        min_buy: bitcoin::Amount,
        max_buy: bitcoin::Amount,
        env_config: env::Config,
        latest_rate: LR,
        resume_only: bool,
        lock_input_verifier: LockInputVerifier,
    ) -> Self {
        Self {
            events: Default::default(),
            min_buy,
            max_buy,
            env_config,
            latest_rate,
            resume_only,
            lock_input_verifier,
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
            self.lock_input_verifier.clone(),
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
            self.lock_input_verifier.clone(),
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
                span,
                send_wallet_snapshot,
                check_lock_inputs,
                register_swap,
            } => self.events.push_back(OutEvent::Initiated {
                span,
                send_wallet_snapshot,
                check_lock_inputs,
                register_swap,
            }),
            // The swap was registered with the ASB through `register_swap`
            HandlerOutEvent::Completed(Ok(_)) => {}
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
    inbound_streams: FuturesUnordered<BoxFuture<'static, Result<Uuid>>>,
    events: VecDeque<HandlerOutEvent>,

    peer_id: PeerId,
    connection_id: ConnectionId,

    min_buy: bitcoin::Amount,
    max_buy: bitcoin::Amount,
    env_config: env::Config,

    latest_rate: LR,
    resume_only: bool,

    lock_input_verifier: LockInputVerifier,

    // This is the timeout for the negotiation phase where Alice and Bob exchange messages
    negotiation_timeout: Duration,
    // This is the timeout for Bob to commit to or abort the negotiated swap
    decision_timeout: Duration,
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
        lock_input_verifier: LockInputVerifier,
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
            lock_input_verifier,
            negotiation_timeout: crate::defaults::NEGOTIATION_TIMEOUT,
            decision_timeout: crate::defaults::SWAP_SETUP_DECISION_TIMEOUT,
        }
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum HandlerOutEvent {
    Initiated {
        /// Span of this swap setup. Work it requests from the event loop is logged under it.
        span: tracing::Span,
        send_wallet_snapshot:
            bmrng::RequestReceiver<bitcoin::Amount, (WalletSnapshot, bitcoin::Amount, bool)>,
        check_lock_inputs: bmrng::RequestReceiver<
            LockInputCheck,
            Result<LockInputReservation, LockInputCheckError>,
        >,
        register_swap: bmrng::RequestReceiver<RegisterSwap, Result<()>>,
    },
    Completed(Result<Uuid>),
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

                let (register_swap_sender, register_swap_receiver) =
                    bmrng::channel::<RegisterSwap, Result<()>>(1);

                let resume_only = self.resume_only;
                let min_buy = self.min_buy;
                let max_buy = self.max_buy;
                let latest_rate = self.latest_rate.latest_rate();
                let env_config = self.env_config;

                // We wrap the handshake in a timeout future
                let negotiation = tokio::time::timeout(
                    self.negotiation_timeout,
                    run_swap_setup(
                        substream,
                        sender,
                        check_lock_inputs_sender,
                        self.lock_input_verifier.clone(),
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
                let peer_id = self.peer_id;
                let decision_timeout = self.decision_timeout;

                // Attach a span so every log emitted during the negotiation is
                // attributable to the peer and connection it belongs to.
                // The swap id is recorded once Bob sent it.
                let span = tracing::info_span!(
                    "swap_setup",
                    peer = %self.peer_id,
                    connection = %self.connection_id,
                    swap_id = tracing::field::Empty,
                );
                let event_span = span.clone();

                let max_seconds = self.negotiation_timeout.as_secs();
                self.inbound_streams.push(
                    async move {
                        tracing::debug!("Inbound swap setup negotiation started");

                        let Ok(negotiated) = negotiation.await else {
                            tracing::warn!(timeout_seconds = max_seconds, "Swap setup timed out");
                            return Err(anyhow!(
                                "Failed to complete execution setup within {}s",
                                max_seconds
                            ));
                        };

                        let result = match negotiated {
                            Ok((swap_id, state3, lock_input_reservation, substream)) => {
                                await_bob_decision(
                                    substream,
                                    register_swap_sender,
                                    RegisterSwap {
                                        peer_id,
                                        swap_id,
                                        state3,
                                        lock_input_reservation,
                                    },
                                    decision_timeout,
                                )
                                .await
                            }
                            Err(error) => Err(error),
                        };

                        match &result {
                            Ok(swap_id) => {
                                tracing::info!(%swap_id, "Swap setup completed")
                            }
                            Err(error) => {
                                tracing::warn!(error = %format!("{error:#}"), "Swap setup failed")
                            }
                        }

                        result
                    }
                    .instrument(span)
                    .boxed(),
                );

                self.events.push_back(HandlerOutEvent::Initiated {
                    span: event_span,
                    send_wallet_snapshot: receiver,
                    check_lock_inputs: check_lock_inputs_receiver,
                    register_swap: register_swap_receiver,
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
    substream: libp2p::swarm::Stream,
    sender: bmrng::RequestSender<bitcoin::Amount, (WalletSnapshot, bitcoin::Amount, bool)>,
    check_lock_inputs: bmrng::RequestSender<
        LockInputCheck,
        Result<LockInputReservation, LockInputCheckError>,
    >,
    lock_input_verifier: LockInputVerifier,
    peer_id: PeerId,
    resume_only: bool,
    env_config: env::Config,
    min_buy: bitcoin::Amount,
    max_buy: bitcoin::Amount,
    latest_rate: Result<swap_feed::Rate, Box<dyn std::error::Error + Send + Sync + 'static>>,
) -> Result<(Uuid, State3, LockInputReservation, swap_setup::SetupStream)> {
    let mut substream = swap_setup::framed(substream);
    let request = swap_setup::read_cbor_message::<SpotPriceRequest>(&mut substream)
        .await
        .context("Failed to read spot price request")?
        .context("Peer sent an error instead of spot price request")?;
    tracing::debug!(btc = %request.btc, "Received spot price request");

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

    let state0 = State0::new(
        request.btc,
        xmr,
        btc_amnesty_amount,
        env_config,
        wallet_snapshot.redeem_address,
        wallet_snapshot.punish_address,
        wallet_snapshot.fee_rate,
        wallet_snapshot.redeem_multiplier,
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
            let rejection = swap_setup::SwapSetupError::try_from(error)?;
            if let Err(write_error) =
                swap_setup::write_cbor_error(&mut substream, rejection.clone()).await
            {
                tracing::error!(error = %write_error, "Couldn't send swap setup rejection to Bob");
            }
            return Err(rejection).context("Failed to accept swap setup");
        }
    };

    tracing::Span::current().record("swap_id", tracing::field::display(swap_id));
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
    tracing::debug!(
        inputs = tx_lock_input_count,
        outputs = message2.tx_lock_psbt.unsigned_tx.output.len(),
        proofs = message2.tx_lock_proofs.len(),
        "Received lock PSBT"
    );
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

    let received = async {
        // Reject malformed PSBTs and drop Bob's extra PSBT fields before queueing.
        let tx_lock_psbt = bitcoin::canonical_received_psbt(message2.tx_lock_psbt)
            .context("Malformed lock PSBT")?;
        anyhow::ensure!(
            message2.tx_lock_proofs.len() == tx_lock_input_count,
            "Got {} ownership proofs for {tx_lock_input_count} lock inputs",
            message2.tx_lock_proofs.len()
        );
        let tx_lock_fee = tx_lock_psbt
            .fee()
            .context("Invalid TxLock input/output amounts")?;
        Ok::<_, anyhow::Error>((tx_lock_psbt, tx_lock_fee))
    }
    .await;
    let (tx_lock_psbt, tx_lock_fee) = match received {
        Ok(received) => received,
        Err(cause) => return Err(reject_lock_inputs(&mut substream, cause).await),
    };

    // The fee uses Bob's claimed prevout values, which the verification job below checks.
    let tx_lock_weight = bitcoin::estimated_signed_lock_weight(&tx_lock_psbt);
    tracing::debug!(fee = %tx_lock_fee, weight = %tx_lock_weight, "Checking lock fee floor");
    if let Err(error) = swap_machine::common::sanity_check_transaction_fee_floor(
        tx_lock_fee,
        tx_lock_weight,
        wallet_snapshot.estimated_fee_rate,
        request.btc,
    ) {
        let rejection = swap_setup::SwapSetupError::try_from(error)?;
        if let Err(write_error) =
            swap_setup::write_cbor_error(&mut substream, rejection.clone()).await
        {
            tracing::error!(error = %write_error, "Couldn't send lock fee rejection to Bob");
        }
        return Err(rejection).context("Lock transaction fee sanity check failed");
    }

    if let Err(cause) = state1.check_tx_lock(&tx_lock_psbt) {
        return Err(reject_lock_inputs(&mut substream, cause).await);
    }

    let verification_started = std::time::Instant::now();
    let verification = async {
        let challenge = state1.ownership_challenge(&tx_lock_psbt);
        let verified = lock_input_verifier
            .verify_lock(
                peer_id,
                tx_lock_psbt,
                message2.tx_lock_proofs,
                challenge,
                tokio::time::Instant::now() + LOCK_INPUT_VERIFICATION_TIMEOUT,
            )
            .await?;
        let psbt = verified.psbt().clone();
        let state2 = state1
            .receive(verified)
            .context("Failed to reconstruct lock transaction")?;
        Ok::<_, anyhow::Error>((state2, psbt))
    }
    .await;
    let (state2, psbt) = match verification {
        Ok(verified) => verified,
        Err(cause) => return Err(reject_lock_inputs(&mut substream, cause).await),
    };

    tracing::debug!(
        duration_ms = verification_started.elapsed().as_millis(),
        "Swap setup transition: State1 -> State2 (verified lock inputs)"
    );

    let lock_input_check = check_lock_inputs
        .send_receive(LockInputCheck { psbt })
        .await
        .unwrap_or_else(|error| {
            Err(LockInputCheckError(format!(
                "Failed to receive lock input check result: {error}"
            )))
        });
    let lock_input_reservation = match lock_input_check {
        Ok(reservation) => reservation,
        Err(cause) => {
            let cause = anyhow::Error::new(cause).context("Lock input check failed");
            return Err(reject_lock_inputs(&mut substream, cause).await);
        }
    };
    tracing::debug!("Reserved lock inputs");

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

    Ok((swap_id, state3, lock_input_reservation, substream))
}

/// Tells Bob that his lock inputs were rejected and returns the cause.
async fn reject_lock_inputs(
    substream: &mut swap_setup::SetupStream,
    cause: anyhow::Error,
) -> anyhow::Error {
    let error = lock_input_verification_error(&cause);
    tracing::debug!(rejection = ?error, "Rejecting the lock inputs of Bob");
    if let Err(err) = swap_setup::write_cbor_error(substream, error).await {
        tracing::error!(error=%err, "Couldn't send lock input verification error to Bob");
    }
    cause.context("Lock transaction verification failed")
}

/// Bob may retry if verification failed on our side or might pass later.
fn lock_input_verification_error(cause: &anyhow::Error) -> swap_setup::SwapSetupError {
    let is_transient = cause.downcast_ref::<LockInputCheckError>().is_some()
        || cause
            .downcast_ref::<VerificationError>()
            .is_some_and(VerificationError::is_transient);
    if is_transient {
        return swap_setup::SwapSetupError::LockInputVerificationInconclusive;
    }
    swap_setup::SwapSetupError::LockInputVerificationFailed
}

/// Keeps the lock inputs reserved until Bob commits to the swap. The swap is
/// persisted before Bob is told that he may publish the lock transaction.
/// If Bob aborts, disconnects or does not decide in time, the reservation is dropped.
async fn await_bob_decision(
    mut substream: swap_setup::SetupStream,
    register_swap: bmrng::RequestSender<RegisterSwap, Result<()>>,
    registration: RegisterSwap,
    decision_timeout: Duration,
) -> Result<Uuid> {
    let swap_id = registration.swap_id;

    let decision = tokio::time::timeout(
        decision_timeout,
        swap_setup::read_cbor_message::<BobDecision>(&mut substream),
    )
    .await
    .with_context(|| format!("Bob did not decide within {}s", decision_timeout.as_secs()))?
    .context("Failed to read the decision of Bob")?
    .context("Peer sent an error instead of the decision")?;

    if decision == BobDecision::Abort {
        anyhow::bail!("Bob aborted the swap before locking the Bitcoin");
    }
    tracing::debug!("Bob committed to the swap, persisting it");

    // No timeout here: a timeout could fire after the swap was persisted. The event
    // loop bounds persisting by `SWAP_SETUP_PERSIST_TIMEOUT` instead and only answers
    // once it knows whether the swap was persisted.
    register_swap
        .send_receive(registration)
        .await
        .context("Failed to register the swap")?
        .context("Failed to persist the swap")?;
    tracing::debug!("Persisted the swap, confirming it to Bob");

    // The swap is persisted and runs from here on, so failing to tell Bob is not
    // a failure of the swap setup. Without the confirmation Bob does not lock.
    if let Err(error) = confirm_persisted(&mut substream).await {
        tracing::warn!(
            %swap_id,
            ?error,
            "Bob might not have received the confirmation of the persisted swap"
        );
    }

    Ok(swap_id)
}

async fn confirm_persisted(substream: &mut swap_setup::SetupStream) -> Result<()> {
    swap_setup::write_cbor_message(&mut *substream, SwapPersisted)
        .await
        .context("Failed to confirm the persisted swap")?;
    SinkExt::close(substream)
        .await
        .context("Failed to close substream after all messages were sent")?;

    Ok(())
}
