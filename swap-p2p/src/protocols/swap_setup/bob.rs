use crate::out_event;
use crate::protocols::swap_setup::{
    protocol, BlockchainNetwork, SpotPriceError, SpotPriceResponse,
};
use anyhow::{Context, Result};
use bitcoin_wallet::BitcoinWallet;
use futures::future::{BoxFuture, OptionFuture};
use futures::AsyncWriteExt;
use futures::FutureExt;
use libp2p::core::upgrade;
use libp2p::swarm::behaviour::ConnectionEstablished;
use libp2p::swarm::dial_opts::{DialOpts, PeerCondition};
use libp2p::swarm::{
    ConnectionClosed, ConnectionDenied, ConnectionHandler, ConnectionHandlerEvent, ConnectionId,
    FromSwarm, NetworkBehaviour, SubstreamProtocol, THandler, THandlerInEvent, THandlerOutEvent,
    ToSwarm,
};
use libp2p::{Multiaddr, PeerId};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;
use swap_core::bitcoin;
use swap_env::env;
use swap_machine::bob::{State0, State2};
use swap_machine::common::{Message1, Message3};
use uuid::Uuid;

use super::{read_cbor_message, write_cbor_message, SpotPriceRequest};

#[allow(missing_debug_implementations)]
pub struct Behaviour {
    env_config: env::Config,
    bitcoin_wallet: Arc<dyn BitcoinWallet>,

    // Queue of swap setup request that haven't been assigned to a connection handler yet
    // (peer_id, swap_id, new_swap)
    new_swaps: VecDeque<(PeerId, Uuid, NewSwap)>,

    // Maintains the list of connections handlers for a specific peer
    //
    // 0. List of connection handlers that are still active but haven't been assigned a swap setup request yet
    // 1. List of connection handlers that have died. Once their death is acknowledged / processed, they are removed from the list
    connection_handlers: HashMap<PeerId, (VecDeque<ConnectionId>, VecDeque<ConnectionId>)>,

    // Queue of completed swaps that we have assigned a connection handler to but where we haven't notified the ConnectionHandler yet
    // We notify the ConnectionHandler by emitting a ConnectionHandlerEvent::NotifyBehaviour event
    assigned_unnotified_swaps: VecDeque<(ConnectionId, PeerId, Uuid, NewSwap)>,

    // Maintains the list of requests that we have sent to a connection handler but haven't yet received a response
    inflight_requests: HashMap<ConnectionId, (Uuid, PeerId)>,

    // Queue of swap setup results that we want to notify the Swarm about
    to_swarm: VecDeque<SwapSetupResult>,

    // Queue of peers that we want to instruct the Swarm to dial
    to_dial: VecDeque<PeerId>,
}

impl Behaviour {
    pub fn new(env_config: env::Config, bitcoin_wallet: Arc<dyn BitcoinWallet>) -> Self {
        Self {
            env_config,
            bitcoin_wallet,
            new_swaps: VecDeque::default(),
            to_swarm: VecDeque::default(),
            assigned_unnotified_swaps: VecDeque::default(),
            inflight_requests: HashMap::default(),
            connection_handlers: HashMap::default(),
            to_dial: VecDeque::default(),
        }
    }

    pub async fn start(&mut self, alice_peer_id: PeerId, swap: NewSwap) {
        tracing::trace!(
            %alice_peer_id,
            ?swap,
            "Queuing new swap setup request inside the Behaviour",
        );

        // TODO: This is a bit redundant because we already have the swap_id in the NewSwap struct
        self.new_swaps
            .push_back((alice_peer_id, swap.swap_id, swap));
        self.to_dial.push_back(alice_peer_id);
    }

    // Returns a mutable reference to the queues of the connection handlers for a specific peer
    fn connection_handlers_mut(
        &mut self,
        peer_id: PeerId,
    ) -> &mut (VecDeque<ConnectionId>, VecDeque<ConnectionId>) {
        self.connection_handlers.entry(peer_id).or_default()
    }

    // Returns a mutable reference to the queues of the connection handlers for a specific peer
    fn alive_connection_handlers_mut(&mut self, peer_id: PeerId) -> &mut VecDeque<ConnectionId> {
        &mut self.connection_handlers_mut(peer_id).0
    }

    // Returns a mutable reference to the queues of the connection handlers for a specific peer
    fn dead_connection_handlers_mut(&mut self, peer_id: PeerId) -> &mut VecDeque<ConnectionId> {
        &mut self.connection_handlers_mut(peer_id).1
    }

    fn known_peers(&self) -> HashSet<PeerId> {
        self.connection_handlers.keys().copied().collect()
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = Handler;
    type ToSwarm = SwapSetupResult;

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        _peer: PeerId,
        _local_addr: &Multiaddr,
        _remote_addr: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(Handler::new(self.env_config, self.bitcoin_wallet.clone()))
    }

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        _peer: PeerId,
        _addr: &Multiaddr,
        _role_override: libp2p::core::Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(Handler::new(self.env_config, self.bitcoin_wallet.clone()))
    }

    fn on_swarm_event(&mut self, event: FromSwarm<'_>) {
        match event {
            FromSwarm::ConnectionEstablished(ConnectionEstablished {
                peer_id,
                connection_id,
                endpoint,
                ..
            }) => {
                tracing::trace!(
                    peer = %peer_id,
                    connection_id = %connection_id,
                    endpoint = ?endpoint,
                    "A new connection handler has been established",
                );

                self.alive_connection_handlers_mut(peer_id)
                    .push_back(connection_id);
            }
            FromSwarm::ConnectionClosed(ConnectionClosed {
                peer_id,
                connection_id,
                ..
            }) => {
                self.dead_connection_handlers_mut(peer_id)
                    .push_back(connection_id);
            }
            _ => {}
        }
    }

    fn on_connection_handler_event(
        &mut self,
        event_peer_id: PeerId,
        connection_id: libp2p::swarm::ConnectionId,
        result: THandlerOutEvent<Self>,
    ) {
        if let Some((swap_id, peer)) = self.inflight_requests.remove(&connection_id) {
            assert_eq!(peer, event_peer_id);

            self.to_swarm.push_back(SwapSetupResult {
                peer,
                swap_id,
                result,
            });
        }
    }

    fn poll(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        // Forward completed swaps from the connection handler to the swarm
        if let Some(completed) = self.to_swarm.pop_front() {
            tracing::trace!(
                peer = %completed.peer,
                "Forwarding completed swap setup from Behaviour to the Swarm",
            );

            return Poll::Ready(ToSwarm::GenerateEvent(completed));
        }

        // Forward any peers that we want to dial to the Swarm
        if let Some(peer) = self.to_dial.pop_front() {
            // TODO: We need to redial here!!
            tracing::trace!(
                peer = %peer,
                "Instructing swarm to dial a new connection handler for a swap setup request",
            );

            return Poll::Ready(ToSwarm::Dial {
                opts: DialOpts::peer_id(peer)
                    .condition(PeerCondition::DisconnectedAndNotDialing)
                    .build(),
            });
        }

        // Remove any unused already dead connection handlers that were never assigned a request
        for peer in self.known_peers() {
            let (alive_connection_handlers, dead_connection_handlers) =
                self.connection_handlers_mut(peer);

            // Create sets for efficient lookup
            let alive_set: HashSet<_> = alive_connection_handlers.iter().copied().collect();
            let dead_set: HashSet<_> = dead_connection_handlers.iter().copied().collect();

            // Remove from alive any handlers that are also in dead
            alive_connection_handlers.retain(|id| !dead_set.contains(id));

            // Remove from dead any handlers that were in alive (the overlap we just processed)
            dead_connection_handlers.retain(|id| !alive_set.contains(id));
        }

        // Go through our new_swaps and try to assign a request to a connection handler
        //
        // If we find a connection handler for the peer, it will be removed from new_swaps
        // If we don't find a connection handler for the peer, it will remain in new_swaps
        {
            let new_swaps = &mut self.new_swaps;
            let connection_handlers = &mut self.connection_handlers;
            let assigned_unnotified_swaps = &mut self.assigned_unnotified_swaps;

            let mut remaining = std::collections::VecDeque::new();
            for (peer, swap_id, new_swap) in new_swaps.drain(..) {
                if let Some(connection_id) =
                // TODO: A connection handler can be used multiple times!!! This will prevent us from using it again!
                    connection_handlers.entry(peer).or_default().0.pop_front()
                {
                    assigned_unnotified_swaps.push_back((connection_id, peer, swap_id, new_swap));
                } else {
                    remaining.push_back((peer, swap_id, new_swap));
                }
            }

            *new_swaps = remaining;
        }

        // If a connection handler died which had an assigned swap setup request,
        // we need to notify the swarm that the request failed
        for peer_id in self.known_peers() {
            while let Some(connection_id) = self.dead_connection_handlers_mut(peer_id).pop_front() {
                if let Some((swap_id, _)) = self.inflight_requests.remove(&connection_id) {
                    self.to_swarm.push_back(SwapSetupResult {
                        peer: peer_id,
                        swap_id,
                        result: Err(anyhow::anyhow!("Connection handler for peer {} has died after we notified it of the swap setup request", peer_id)),
                    });
                }
            }
        }

        // Iterate through our assigned_unnotified_swaps queue (with popping)
        if let Some((connection_id, peer_id, swap_id, new_swap)) =
            self.assigned_unnotified_swaps.pop_front()
        {
            tracing::trace!(
                swap_id = %swap_id,
                connection_id = %connection_id,
                ?new_swap,
                "Dispatching swap setup request from Behaviour to a specific connection handler",
            );

            // Check if the connection handler is still alive
            if let Some(dead_connection_handler) = self
                .dead_connection_handlers_mut(peer_id)
                .iter()
                .position(|id| *id == connection_id)
            {
                self.dead_connection_handlers_mut(peer_id)
                    .remove(dead_connection_handler);

                self.to_swarm.push_back(SwapSetupResult {
                    peer: peer_id,
                    swap_id,
                    result: Err(anyhow::anyhow!("Connection handler for peer {} has died before we could notify it of the swap setup request", peer_id)),
                });
            } else {
                // ConnectionHandler must still be alive, notify it of the swap setup request
                tracing::trace!(
                    peer = %peer_id,
                    swap_id = %swap_id,
                    ?new_swap,
                    "Notifying connection handler of the swap setup request. We are assuming it is still alive.",
                );

                self.inflight_requests
                    .insert(connection_id, (swap_id, peer_id));

                return Poll::Ready(ToSwarm::NotifyHandler {
                    peer_id,
                    handler: libp2p::swarm::NotifyHandler::One(connection_id),
                    event: new_swap,
                });
            }
        }

        Poll::Pending
    }
}

type OutboundStream = BoxFuture<'static, Result<State2, Error>>;

// TODO: A single connection handler can be used multiple times!!!
pub struct Handler {
    outbound_stream: OptionFuture<OutboundStream>,
    env_config: env::Config,
    timeout: Duration,
    new_swaps: VecDeque<NewSwap>,
    bitcoin_wallet: Arc<dyn BitcoinWallet>,
    keep_alive: bool, // TODO:; This needs to be a little bit more granular to support multiple swaps on the same connection (differnet substreams)
}

impl Handler {
    fn new(env_config: env::Config, bitcoin_wallet: Arc<dyn BitcoinWallet>) -> Self {
        Self {
            env_config,
            outbound_stream: OptionFuture::from(None),
            timeout: crate::defaults::NEGOTIATION_TIMEOUT,
            new_swaps: VecDeque::default(),
            bitcoin_wallet,
            // TODO: This will keep ALL connections alive indefinitely
            // which is not optimal
            keep_alive: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NewSwap {
    pub swap_id: Uuid,
    pub btc: bitcoin::Amount,
    pub tx_lock_fee: bitcoin::Amount,
    pub tx_refund_fee: bitcoin::Amount,
    pub tx_cancel_fee: bitcoin::Amount,
    pub bitcoin_refund_address: bitcoin::Address,
}

#[derive(Debug)]
pub struct SwapSetupResult {
    peer: PeerId,
    swap_id: Uuid,
    result: Result<State2>,
}

impl ConnectionHandler for Handler {
    type FromBehaviour = NewSwap;
    type ToBehaviour = Result<State2>;
    type InboundProtocol = upgrade::DeniedUpgrade;
    type OutboundProtocol = protocol::SwapSetup;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = NewSwap;

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        // Bob does not support inbound substreams
        SubstreamProtocol::new(upgrade::DeniedUpgrade, ())
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
            libp2p::swarm::handler::ConnectionEvent::FullyNegotiatedInbound(_) => {
                // TODO: Maybe warn here as Bob does not support inbound substreams?
            }
            libp2p::swarm::handler::ConnectionEvent::FullyNegotiatedOutbound(outbound) => {
                let mut substream = outbound.protocol;
                let new_swap_request = outbound.info;

                let bitcoin_wallet = self.bitcoin_wallet.clone();
                let env_config = self.env_config;

                let protocol = tokio::time::timeout(self.timeout, async move {
                    let result = run_swap_setup(
                        &mut substream,
                        new_swap_request,
                        env_config,
                        bitcoin_wallet,
                    )
                    .await;

                    result.map_err(|err: anyhow::Error| {
                        tracing::error!(?err, "Error occurred during swap setup protocol");
                        Error::Protocol(format!("{:?}", err))
                    })
                });

                let max_seconds = self.timeout.as_secs();

                self.outbound_stream = OptionFuture::from(Some(Box::pin(async move {
                    protocol.await.map_err(|_| Error::Timeout {
                        seconds: max_seconds,
                    })?
                })
                    as OutboundStream));
            }
            libp2p::swarm::handler::ConnectionEvent::AddressChange(address_change) => {
                tracing::trace!(
                    ?address_change,
                    "Connection address changed during swap setup"
                );
            }
            libp2p::swarm::handler::ConnectionEvent::DialUpgradeError(dial_upgrade_error) => {
                tracing::trace!(error = %dial_upgrade_error.error, "Dial upgrade error during swap setup");
            }
            libp2p::swarm::handler::ConnectionEvent::ListenUpgradeError(listen_upgrade_error) => {
                tracing::trace!(
                    ?listen_upgrade_error,
                    "Listen upgrade error during swap setup"
                );
            }
            _ => {
                // We ignore the rest of events
            }
        }
    }

    fn on_behaviour_event(&mut self, new_swap: Self::FromBehaviour) {
        self.new_swaps.push_back(new_swap);
    }

    fn connection_keep_alive(&self) -> bool {
        self.keep_alive
    }

    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::ToBehaviour>,
    > {
        // Check if there is a new swap to be started on this connection
        // Has the Behaviour assigned us a new swap to be started on this connection?
        if let Some(new_swap) = self.new_swaps.pop_front() {
            tracing::trace!(
                ?new_swap.swap_id,
                "Instructing swarm to start a new outbound substream as part of swap setup",
            );

            // Keep the connection alive because we want to use it
            self.keep_alive = true;

            // We instruct the swarm to start a new outbound substream
            return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(protocol::new(), new_swap),
            });
        }

        // Check if the outbound stream has completed
        if let Poll::Ready(Some(result)) = self.outbound_stream.poll_unpin(cx) {
            self.outbound_stream = None.into();

            // Once the outbound stream is completed, we no longer keep the connection alive
            self.keep_alive = false;

            // We notify the swarm that the swap setup is completed / failed
            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                result.map_err(anyhow::Error::from).into(),
            ));
        }

        Poll::Pending
    }
}

async fn run_swap_setup(
    mut substream: &mut libp2p::swarm::Stream,
    new_swap_request: NewSwap,
    env_config: env::Config,
    bitcoin_wallet: Arc<dyn BitcoinWallet>,
) -> Result<State2> {
    // Here we request the spot price from Alice
    write_cbor_message(
        &mut substream,
        SpotPriceRequest {
            btc: new_swap_request.btc,
            blockchain_network: BlockchainNetwork {
                bitcoin: env_config.bitcoin_network,
                monero: env_config.monero_network,
            },
        },
    )
    .await
    .context("Failed to send spot price request to Alice")?;

    // Here we read the spot price response from Alice
    // The outer ? checks if Alice responded with an error (SpotPriceError)
    let xmr = Result::from(
        // The inner ? is for the read_cbor_message function
        // It will return an error if the deserialization fails
        read_cbor_message::<SpotPriceResponse>(&mut substream)
            .await
            .context("Failed to read spot price response from Alice")?,
    )?;

    tracing::trace!(
        %new_swap_request.swap_id,
        xmr = %xmr,
        btc = %new_swap_request.btc,
        "Got spot price response from Alice as part of swap setup",
    );

    let state0 = State0::new(
        new_swap_request.swap_id,
        &mut rand::thread_rng(),
        new_swap_request.btc,
        xmr,
        env_config.bitcoin_cancel_timelock.into(),
        env_config.bitcoin_punish_timelock.into(),
        new_swap_request.bitcoin_refund_address.clone(),
        env_config.monero_finality_confirmations,
        new_swap_request.tx_refund_fee,
        new_swap_request.tx_cancel_fee,
        new_swap_request.tx_lock_fee,
    );

    tracing::trace!(
        %new_swap_request.swap_id,
        "Transitioned into state0 during swap setup",
    );

    write_cbor_message(&mut substream, state0.next_message())
        .await
        .context("Failed to send state0 message to Alice")?;
    let message1 = read_cbor_message::<Message1>(&mut substream)
        .await
        .context("Failed to read message1 from Alice")?;
    let state1 = state0
        .receive(bitcoin_wallet.as_ref(), message1)
        .await
        .context("Failed to receive state1")?;

    tracing::trace!(
        %new_swap_request.swap_id,
        "Transitioned into state1 during swap setup",
    );

    write_cbor_message(&mut substream, state1.next_message())
        .await
        .context("Failed to send state1 message")?;
    let message3 = read_cbor_message::<Message3>(&mut substream)
        .await
        .context("Failed to read message3 from Alice")?;
    let state2 = state1
        .receive(message3)
        .context("Failed to receive state2")?;

    tracing::trace!(
        %new_swap_request.swap_id,
        "Transitioned into state2 during swap setup",
    );

    write_cbor_message(&mut substream, state2.next_message())
        .await
        .context("Failed to send state2 message")?;

    substream
        .flush()
        .await
        .context("Failed to flush substream")?;
    substream
        .close()
        .await
        .context("Failed to close substream")?;

    tracing::trace!(
        %new_swap_request.swap_id,
        "Swap setup completed",
    );

    Ok(state2)
}

impl From<SpotPriceResponse> for Result<swap_core::monero::Amount, Error> {
    fn from(response: SpotPriceResponse) -> Self {
        match response {
            SpotPriceResponse::Xmr(amount) => Ok(amount),
            SpotPriceResponse::Error(e) => Err(e.into()),
        }
    }
}

#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("Seller currently does not accept incoming swap requests, please try again later")]
    NoSwapsAccepted,
    #[error("Seller refused to buy {buy} because the minimum configured buy limit is {min}")]
    AmountBelowMinimum {
        min: bitcoin::Amount,
        buy: bitcoin::Amount,
    },
    #[error("Seller refused to buy {buy} because the maximum configured buy limit is {max}")]
    AmountAboveMaximum {
        max: bitcoin::Amount,
        buy: bitcoin::Amount,
    },
    #[error("Seller's XMR balance is currently too low to fulfill the swap request to buy {buy}, please try again later")]
    BalanceTooLow { buy: bitcoin::Amount },

    #[error("Seller blockchain network {asb:?} setup did not match your blockchain network setup {cli:?}")]
    BlockchainNetworkMismatch {
        cli: BlockchainNetwork,
        asb: BlockchainNetwork,
    },

    #[error("Failed to complete swap setup within {seconds}s")]
    Timeout { seconds: u64 },

    /// Something went wrong during the swap setup protocol that is not covered by the other errors
    /// but where we have some context about the error
    #[error("Something went wrong during the swap setup protocol: {0}")]
    Protocol(String),

    /// To be used for errors that cannot be explained on the CLI side (e.g.
    /// rate update problems on the seller side)
    #[error("Seller encountered a problem, please try again later.")]
    Other,
}

impl From<SpotPriceError> for Error {
    fn from(error: SpotPriceError) -> Self {
        match error {
            SpotPriceError::NoSwapsAccepted => Error::NoSwapsAccepted,
            SpotPriceError::AmountBelowMinimum { min, buy } => {
                Error::AmountBelowMinimum { min, buy }
            }
            SpotPriceError::AmountAboveMaximum { max, buy } => {
                Error::AmountAboveMaximum { max, buy }
            }
            SpotPriceError::BalanceTooLow { buy } => Error::BalanceTooLow { buy },
            SpotPriceError::BlockchainNetworkMismatch { cli, asb } => {
                Error::BlockchainNetworkMismatch { cli, asb }
            }
            SpotPriceError::Other => Error::Other,
        }
    }
}

impl From<SwapSetupResult> for out_event::bob::OutEvent {
    fn from(completed: SwapSetupResult) -> Self {
        out_event::bob::OutEvent::SwapSetupCompleted {
            result: Box::new(completed.result),
            swap_id: completed.swap_id,
            peer: completed.peer,
        }
    }
}
