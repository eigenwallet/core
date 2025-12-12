use crate::futures_util::FuturesHashSet;
use crate::out_event;
use crate::protocols::swap_setup::{
    BlockchainNetwork, SpotPriceError, SpotPriceResponse, protocol,
};
use anyhow::{Context, Result};
use bitcoin_wallet::BitcoinWallet;
use futures::AsyncWriteExt;
use futures::FutureExt;
use futures::future::{BoxFuture, OptionFuture};
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

use super::{SpotPriceRequest, read_cbor_message, write_cbor_message};

// TODO: This should use redial::Behaviour to keep connections alive for peers with queued requests
// TODO: Do not use swap_id as key inside the ConnectionHandler, use another key
#[allow(missing_debug_implementations)]
pub struct Behaviour {
    env_config: env::Config,
    bitcoin_wallet: Arc<dyn BitcoinWallet>,

    // Queue of swap setup request that haven't been assigned to a connection handler yet
    // (peer_id, swap_id, new_swap)
    new_swaps: VecDeque<(PeerId, NewSwap)>,

    // Maintains the set of all alive connections handlers for a specific peer
    connection_handlers: HashMap<PeerId, HashSet<ConnectionId>>,
    connection_handler_deaths: VecDeque<(PeerId, ConnectionId)>,

    // Queue of completed swaps that we have assigned a connection handler to but where we haven't notified the ConnectionHandler yet
    // We notify the ConnectionHandler by emitting a ConnectionHandlerEvent::NotifyBehaviour event
    assigned_unnotified_swaps: VecDeque<(ConnectionId, PeerId, Uuid, NewSwap)>,

    // Maintains the list of requests that we have sent to a connection handler but haven't yet received a response
    inflight_requests: HashMap<ConnectionId, HashSet<(Uuid, PeerId)>>,

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
            connection_handler_deaths: VecDeque::default(),
            to_dial: VecDeque::default(),
        }
    }

    pub fn queue_new_swap(&mut self, alice_peer_id: PeerId, swap: NewSwap) {
        tracing::trace!(
            %alice_peer_id,
            ?swap,
            "Queuing new swap setup request inside the Behaviour",
        );

        self.new_swaps.push_back((alice_peer_id, swap));
        self.to_dial.push_back(alice_peer_id);
    }

    // Returns a mutable reference to the queues of the connection handlers for a specific peer
    fn connection_handlers_mut(&mut self, peer_id: PeerId) -> &mut HashSet<ConnectionId> {
        self.connection_handlers.entry(peer_id).or_default()
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
        // This should never be called as Bob does not support inbound substreams
        // TODO: Can this still be called somehow by libp2p? Can we forbid this?
        debug_assert!(
            false,
            "Bob does not listen so he should never get an inbound connection"
        );

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

                self.connection_handlers_mut(peer_id).insert(connection_id);
            }
            FromSwarm::ConnectionClosed(ConnectionClosed {
                peer_id,
                connection_id,
                ..
            }) => {
                tracing::trace!(
                    peer = %peer_id,
                    connection_id = %connection_id,
                    "A connection handler has died",
                );

                self.connection_handler_deaths
                    .push_back((peer_id, connection_id));
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
        let (handler_swap_id, result) = result;

        if self
            .inflight_requests
            .get_mut(&connection_id)
            .map(|swap_ids| swap_ids.remove(&(handler_swap_id, event_peer_id)))
            .unwrap_or(false)
        {
            self.to_swarm.push_back(SwapSetupResult {
                peer: event_peer_id,
                swap_id: handler_swap_id,
                result,
            });
        } else {
            debug_assert!(
                false,
                "Received a swap setup result from a connection handler for which we have no inflight request stored"
            );
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

        // Check for dead connection handlers
        // Important: This must be done at the top of the function to avoid assigning new swaps to dead connection handlers
        while let Some((peer_id, connection_id)) = self.connection_handler_deaths.pop_front() {
            // Did the connection handler have any assigned swap setup request?
            // If it did, we need to notify the swarm that the request failed
            if let Some(swap_ids) = self.inflight_requests.remove(&connection_id) {
                for (swap_id, peer_id) in swap_ids {
                    self.to_swarm.push_back(SwapSetupResult {
                        peer: peer_id,
                        swap_id,
                        result: Err(anyhow::anyhow!("Connection handler for peer died after we notified it of the swap setup request")),
                    });
                }
            }

            // After handling inflight request, remove the connection handler from the list
            self.connection_handlers
                .get_mut(&peer_id)
                .map(|connection_ids| connection_ids.remove(&connection_id));
        }

        self.new_swaps.retain(|(peer, new_swap)| {
            // Check if we have any open connection handlers for this peer
            if let Some(connection_ids) = self.connection_handlers.get(&peer) {
                // Choose the first one and assign it to the new swap
                if let Some(connection_id) = connection_ids.iter().next() {
                    // TODO: Double swap_id is useless
                    self.assigned_unnotified_swaps.push_back((
                        *connection_id,
                        peer.clone(),
                        new_swap.swap_id.clone(),
                        new_swap.clone(),
                    ));

                    // Remove the swap from queue
                    return false;
                }
            }

            // Keep in queue as we didn't find a connection handler for this peer
            true
        });

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

            // ConnectionHandler must still be alive
            // If it wasn't we'd have removed it from the list at the start of poll(..)
            tracing::trace!(
                peer = %peer_id,
                swap_id = %swap_id,
                ?new_swap,
                "Notifying connection handler of the swap setup request. We are assuming it is still alive.",
            );

            self.inflight_requests
                .entry(connection_id)
                .or_default()
                .insert((swap_id, peer_id));

            return Poll::Ready(ToSwarm::NotifyHandler {
                peer_id,
                handler: libp2p::swarm::NotifyHandler::One(connection_id),
                event: new_swap,
            });
        }

        Poll::Pending
    }
}

pub struct Handler {
    // Configuration
    env_config: env::Config,
    timeout: Duration,
    bitcoin_wallet: Arc<dyn BitcoinWallet>,

    // Queue of swap setup requests that do not have an inflight substream negotiation
    new_swaps: VecDeque<NewSwap>,

    // When we have instructed the Behaviour to start a new outbound substream, we store the swap id here
    // Eventually we will either get a fully negotiated outbound substream or a dial upgrade error
    inflight_substream_negotiations: HashSet<Uuid>,

    // Inflight swap setup requests that we have a fully negotiated outbound substream for
    outbound_streams: FuturesHashSet<Uuid, Result<State2, Error>>,

    // Queue of swap setup results that we want to notify the Behaviour about
    to_behaviour: VecDeque<(Uuid, Result<State2>)>,
}

impl Handler {
    fn new(env_config: env::Config, bitcoin_wallet: Arc<dyn BitcoinWallet>) -> Self {
        Self {
            env_config,
            timeout: crate::defaults::NEGOTIATION_TIMEOUT,
            bitcoin_wallet,
            new_swaps: VecDeque::default(),
            outbound_streams: FuturesHashSet::default(),
            to_behaviour: VecDeque::default(),
            inflight_substream_negotiations: HashSet::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct NewSwap {
    pub swap_id: Uuid,
    pub btc: bitcoin::Amount,
    pub tx_lock_fee: bitcoin::Amount,
    pub tx_refund_fee: bitcoin::Amount,
    pub tx_partial_refund_fee: bitcoin::Amount,
    pub tx_refund_amnesty_fee: bitcoin::Amount,
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
    type ToBehaviour = (Uuid, Result<State2>);
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
            libp2p::swarm::handler::ConnectionEvent::FullyNegotiatedOutbound(
                libp2p::swarm::handler::FullyNegotiatedOutbound {
                    protocol: mut substream,
                    info,
                },
            ) => {
                let swap_id = info.swap_id;

                // We got the substream, so its no longer inflight
                self.inflight_substream_negotiations.remove(&swap_id);

                let bitcoin_wallet = self.bitcoin_wallet.clone();
                let env_config = self.env_config;

                // This runs runs the actual negotiation protocol
                // It is wrapped in a timeout to protect against the case where the peer does not respond
                let protocol = tokio::time::timeout(self.timeout, async move {
                    let result =
                        run_swap_setup(&mut substream, info, env_config, bitcoin_wallet).await;

                    result.map_err(|err: anyhow::Error| {
                        tracing::error!(?err, "Error occurred during swap setup protocol");
                        Error::Protocol(format!("{:?}", err))
                    })
                });

                let max_seconds = self.timeout.as_secs();

                let did_replace_existing_future = self.outbound_streams.replace(
                    swap_id,
                    Box::pin(async move {
                        protocol.await.map_err(|_| Error::Timeout {
                            seconds: max_seconds,
                        })?
                    }),
                );

                // In poll(..), we ensure that we never dispatch multiple concurrent swap setup requests for the same swap on the same ConnectionHandler
                // This invariant should therefore never be violated
                // TODO: Is this truly true?
                assert!(
                    !did_replace_existing_future,
                    "Replacing an existing inflight swap setup request is not allowed. We should have checked for this invariant before instructing the Behaviour to start a substream."
                );
            }
            libp2p::swarm::handler::ConnectionEvent::DialUpgradeError(
                libp2p::swarm::handler::DialUpgradeError { info, error },
            ) => {
                // We failed to get a fully negotiated outbound substream, so its no longer inflight
                self.inflight_substream_negotiations.remove(&info.swap_id);

                tracing::error!(%error, "Dial upgrade error during swap setup substream negotiation. Propagating error back to the Behaviour");

                self.to_behaviour.push_back((
                    info.swap_id,
                    Err(anyhow::Error::from(error)
                        .context("Dial upgrade error during swap setup. The peer may not support the swap setup protocol.")),
                ));
            }
            libp2p::swarm::handler::ConnectionEvent::ListenUpgradeError(_)
            | libp2p::swarm::handler::ConnectionEvent::FullyNegotiatedInbound(_) => {
                // This should never be called as Bob does not support inbound substreams
                // TODO: Maybe warn here as Bob does not support inbound substreams?
                debug_assert!(
                    false,
                    "Bob does not support inbound substreams for the swap setup protocol"
                );
            }
            _ => {}
        }
    }

    fn on_behaviour_event(&mut self, new_swap: Self::FromBehaviour) {
        tracing::trace!(
            swap_id = %new_swap.swap_id,
            "Received a new swap setup request from the Behaviour",
        );

        self.new_swaps.push_back(new_swap);
    }

    fn connection_keep_alive(&self) -> bool {
        // Keep alive as long as there are queued swaps our inflight requests
        !self.new_swaps.is_empty() || self.outbound_streams.len() > 0
    }

    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::ToBehaviour>,
    > {
        // Check if there is a new swap to be started on this connection
        // Has the Behaviour assigned us a new swap to be started on this connection?
        while let Some(new_swap) = self.new_swaps.pop_front() {
            // Check if we already have an inflight request for this swap
            // We disallow multiple concurrent swap setup requests for the same swap on the same ConnectionHandler
            if self.outbound_streams.contains_key(&new_swap.swap_id) {
                tracing::error!(
                    swap_id = %new_swap.swap_id,
                    "Received a new swap setup request for a swap id that we already have an inflight request for. Ignoring request. The upstream behaviour may encounter bugs if its internal logic does not handle this correctly.",
                );

                // TODO: Potentially make this a production assert
                debug_assert!(
                    false,
                    "Multiple concurrent swap setup requests with the same swap id are not allowed."
                );

                continue;
            }

            // We disallow multiple concurrent substream negotiations for the same swap on the same ConnectionHandler
            if self
                .inflight_substream_negotiations
                .contains(&new_swap.swap_id)
            {
                tracing::error!(
                    swap_id = %new_swap.swap_id,
                    "Received a new swap setup request for a swap id that we already have an inflight substream negotiation for. Ignoring. The upstream behaviour may encounter bugs if its internal logic does not handle this correctly.",
                );

                // TODO: Potentially make this a production assert
                debug_assert!(
                    false,
                    "Multiple concurrent substream negotiations for the same swap id are not allowed."
                );

                continue;
            }

            tracing::trace!(
                ?new_swap.swap_id,
                "Instructing swarm to start a new outbound substream as part of swap setup",
            );

            // We instruct the swarm to start a new outbound substream
            self.inflight_substream_negotiations
                .insert(new_swap.swap_id);

            return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(protocol::new(), new_swap),
            });
        }

        // Check if the outbound stream has completed
        while let Poll::Ready(Some((swap_id, result))) = self.outbound_streams.poll_next_unpin(cx) {
            self.to_behaviour
                .push_back((swap_id, result.map_err(anyhow::Error::from)));
        }

        // Notify the Behaviour about any swap setup results
        if let Some(result) = self.to_behaviour.pop_front() {
            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(result));
        }

        Poll::Pending
    }
}

// TODO: This is protocol and should be moved to another crate (probably swap-machine, swap-core or swap)
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
        new_swap_request.tx_partial_refund_fee,
        new_swap_request.tx_refund_amnesty_fee,
        new_swap_request.tx_refund_fee,
        new_swap_request.tx_cancel_fee,
        new_swap_request.tx_lock_fee,
    );

    tracing::trace!(
        %new_swap_request.swap_id,
        "Transitioned into state0 during swap setup",
    );

    write_cbor_message(
        &mut substream,
        state0
            .next_message()
            .context("Couldn't generate Message0")?,
    )
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

    write_cbor_message(
        &mut substream,
        state2
            .next_message()
            .context("Couldn't construct Message4")?,
    )
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
    #[error(
        "Seller's XMR balance is currently too low to fulfill the swap request to buy {buy}, please try again later"
    )]
    BalanceTooLow { buy: bitcoin::Amount },

    #[error(
        "Seller blockchain network {asb:?} setup did not match your blockchain network setup {cli:?}"
    )]
    BlockchainNetworkMismatch {
        cli: BlockchainNetwork,
        asb: BlockchainNetwork,
    },

    #[error("Failed to complete swap setup back-and-forth within {seconds}s")]
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

// TODO: Tests
// - Case where Alice does not support the protocol at all
// - Case where Connection dies before the swap setup is started
// - Case where Connection dies during the swap setup protocol
// TODO: Extract actualy protocol logic into a callback of sorts or some type of event/state system
