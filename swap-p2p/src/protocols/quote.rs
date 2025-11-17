use std::time::Duration;

use crate::out_event;
use libp2p::request_response::{self, ProtocolSupport};
use libp2p::{PeerId, StreamProtocol};
use serde::{Deserialize, Serialize};
use swap_core::bitcoin;
use typeshare::typeshare;

const PROTOCOL: &str = "/comit/xmr/btc/bid-quote/1.0.0";
pub type OutEvent = request_response::Event<(), BidQuote>;
pub type Message = request_response::Message<(), BidQuote>;

pub type Behaviour = request_response::json::Behaviour<(), BidQuote>;

#[derive(Debug, Clone, Copy, Default)]
pub struct BidQuoteProtocol;

impl AsRef<str> for BidQuoteProtocol {
    fn as_ref(&self) -> &str {
        PROTOCOL
    }
}

/// Represents a quote for buying XMR.
#[derive(Serialize, Deserialize, Debug, Copy, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[typeshare]
pub struct BidQuote {
    /// The price at which the maker is willing to buy at.
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    #[typeshare(serialized_as = "number")]
    pub price: bitcoin::Amount,
    /// The minimum quantity the maker is willing to buy.
    ///     #[typeshare(serialized_as = "number")]
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    #[typeshare(serialized_as = "number")]
    pub min_quantity: bitcoin::Amount,
    /// The maximum quantity the maker is willing to buy.
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    #[typeshare(serialized_as = "number")]
    pub max_quantity: bitcoin::Amount,
}

impl BidQuote {
    /// A zero quote with all amounts set to zero
    pub const ZERO: Self = Self {
        price: bitcoin::Amount::ZERO,
        min_quantity: bitcoin::Amount::ZERO,
        max_quantity: bitcoin::Amount::ZERO,
    };
}

#[derive(Clone, Copy, Debug, thiserror::Error)]
#[error("Received quote of 0")]
pub struct ZeroQuoteReceived;

/// Constructs a new instance of the `quote` behaviour to be used by the ASB.
///
/// The ASB is always listening and only supports inbound connections, i.e.
/// handing out quotes.
pub fn asb() -> Behaviour {
    Behaviour::new(
        vec![(StreamProtocol::new(PROTOCOL), ProtocolSupport::Inbound)],
        request_response::Config::default().with_request_timeout(Duration::from_secs(60)),
    )
}

/// Constructs a new instance of the `quote` behaviour to be used by the CLI.
///
/// The CLI is always dialing and only supports outbound connections, i.e.
/// requesting quotes.
pub fn cli() -> Behaviour {
    Behaviour::new(
        vec![(StreamProtocol::new(PROTOCOL), ProtocolSupport::Outbound)],
        request_response::Config::default().with_request_timeout(Duration::from_secs(60)),
    )
}

impl From<(PeerId, Message)> for out_event::alice::OutEvent {
    fn from((peer, message): (PeerId, Message)) -> Self {
        match message {
            Message::Request { channel, .. } => Self::QuoteRequested { channel, peer },
            Message::Response { .. } => Self::unexpected_response(peer),
        }
    }
}
crate::impl_from_rr_event!(OutEvent, out_event::alice::OutEvent, PROTOCOL);

impl From<(PeerId, Message)> for out_event::bob::OutEvent {
    fn from((peer, message): (PeerId, Message)) -> Self {
        match message {
            Message::Request { .. } => Self::unexpected_request(peer),
            Message::Response {
                response,
                request_id,
            } => Self::QuoteReceived {
                id: request_id,
                response,
            },
        }
    }
}
crate::impl_from_rr_event!(OutEvent, out_event::bob::OutEvent, PROTOCOL);

/// Behaviour that listens for peers that support the protocol and then periodically requests a quote from them.
pub mod background {
    use backoff::{backoff::Backoff, ExponentialBackoff};
    use libp2p::{
        core::Endpoint,
        swarm::{
            ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, THandlerInEvent,
            THandlerOutEvent, ToSwarm,
        },
        Multiaddr, PeerId, StreamProtocol,
    };

    use std::{collections::{HashMap, VecDeque}, task::Poll, time::Duration};

    use super::BidQuote;
    use crate::{
        behaviour_util::ConnectionTracker,
        futures_util::FuturesHashSet,
        protocols::{notice, quote},
    };

    const QUOTE_INTERVAL: Duration = Duration::from_secs(5);

    // TODO: Track which peers support our protocol based on:
    // 1. The notice behaviour / identify protocol
    // 2. OutboundFailure::UnsupportedProtocols errors
    //
    // We should initially assume all peers support our protocol.
    pub struct Behaviour {
        inner: InnerBehaviour,

        /// Track connected peers
        connection_tracker: ConnectionTracker,

        /// Peers to dispatch a quote request to as soon as we are connected to them
        to_dispatch: VecDeque<PeerId>,
        /// Peers to request a quote from once the future resolves
        to_request: FuturesHashSet<PeerId, ()>,

        /// Store backoffs for each peer
        backoff: HashMap<PeerId, ExponentialBackoff>,

        // Queue of events to be sent to the swarm
        to_swarm: VecDeque<Event>,
    }

    impl Behaviour {
        pub fn new() -> Self {
            Self {
                inner: InnerBehaviour {
                    quote: quote::cli(),
                    notice: notice::Behaviour::new(StreamProtocol::new(quote::PROTOCOL)),
                },
                connection_tracker: ConnectionTracker::new(),
                to_dispatch: VecDeque::new(),
                to_request: FuturesHashSet::new(),
                backoff: HashMap::new(),
                to_swarm: VecDeque::new(),
            }
        }

        fn is_connected(&self, peer_id: &PeerId) -> bool {
            self.connection_tracker.is_connected(peer_id)
        }

        pub fn get_backoff(&mut self, peer: &PeerId) -> &mut ExponentialBackoff {
            self.backoff
                .entry(*peer)
                .or_insert_with(|| ExponentialBackoff {
                    initial_interval: Duration::from_secs(1),
                    current_interval: Duration::from_secs(1),
                    max_interval: Duration::from_secs(10),
                    max_elapsed_time: None,
                    ..ExponentialBackoff::default()
                })
        }

        fn schedule_quote_request_after(&mut self, peer: PeerId, duration: Duration) -> Duration {
            self.to_request
                .insert(peer, Box::pin(tokio::time::sleep(duration)));
            duration
        }

        fn schedule_quote_request_with_backoff(&mut self, peer: PeerId) -> Duration {
            let duration = self.get_backoff(&peer).current_interval;

            self.schedule_quote_request_after(peer, duration)
        }

        fn schedule_quote_request_immediately(&mut self, peer: PeerId) {
            self.schedule_quote_request_after(peer, Duration::ZERO);
        }
    }

    #[derive(NetworkBehaviour)]
    pub struct InnerBehaviour {
        quote: quote::Behaviour,
        notice: notice::Behaviour,
    }

    #[derive(Debug)]
    pub enum Event {
        QuoteReceived { peer: PeerId, quote: BidQuote },
    }

    impl libp2p::swarm::NetworkBehaviour for Behaviour {
        type ConnectionHandler =
            <InnerBehaviour as libp2p::swarm::NetworkBehaviour>::ConnectionHandler;
        type ToSwarm = Event;

        fn poll(
            &mut self,
            cx: &mut std::task::Context<'_>,
        ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
            while let Poll::Ready(Some((peer, ()))) = self.to_request.poll_next_unpin(cx) {
                self.to_dispatch.push_back(peer);
            }

            // Only dispatch to connected peers, keep non-connected ones in queue
            // Take ownership of the queue to avoid borrow checker issues
            let to_dispatch = std::mem::take(&mut self.to_dispatch);
            self.to_dispatch = to_dispatch
                .into_iter()
                .filter(|peer| {
                    if self.is_connected(peer) {
                        let outbound_request_id = self.inner.quote.send_request(peer, ());
                        tracing::trace!(
                            %peer,
                            %outbound_request_id,
                            "Dispatching outgoing quote request to peer"
                        );

                        false
                    } else {
                        true
                    }
                })
                .collect();

            let inner_poll = self.inner.poll(cx);

            if let Poll::Ready(ToSwarm::GenerateEvent(event)) = inner_poll {
                match event {
                    InnerBehaviourEvent::Notice(notice::Event::SupportsProtocol { peer }) => {
                        tracing::trace!(%peer, "Queuing quote request to peer after noticing that they support the quote protocol");
                        self.schedule_quote_request_immediately(peer);
                    }
                    InnerBehaviourEvent::Quote(quote::request_response::Event::Message {
                        peer,
                        message,
                    }) => {
                        if let quote::request_response::Message::Response { response, .. } = message
                        {
                            self.to_swarm.push_back(Event::QuoteReceived {
                                peer,
                                quote: response,
                            });

                            // We got a successful response, so we reset the backoff
                            self.get_backoff(&peer).reset();

                            // Schedule a new quote request after backoff
                            self.schedule_quote_request_after(peer, QUOTE_INTERVAL);
                        }
                    }
                    InnerBehaviourEvent::Quote(
                        quote::request_response::Event::OutboundFailure {
                            peer,
                            request_id,
                            error,
                        },
                    ) => {
                        // We got an outbound failure, so we increment the backoff
                        self.get_backoff(&peer)
                            .next_backoff()
                            .expect("backoff should never run out of attempts");

                        // We schedule a new quote request
                        let next_request_in = self.schedule_quote_request_with_backoff(peer);

                        tracing::trace!(%peer, %request_id, %error, next_request_in = %next_request_in.as_secs(), "Queuing quote request to peer after outbound failure");
                    }
                    _ => {
                        // Ignore other events
                    }
                }
            }

            while let Some(event) = self.to_swarm.pop_front() {
                return Poll::Ready(ToSwarm::GenerateEvent(event));
            }

            Poll::Pending
        }

        fn handle_pending_outbound_connection(
            &mut self,
            connection_id: libp2p::swarm::ConnectionId,
            maybe_peer: Option<PeerId>,
            addresses: &[Multiaddr],
            effective_role: libp2p::core::Endpoint,
        ) -> Result<Vec<Multiaddr>, libp2p::swarm::ConnectionDenied> {
            self.inner.handle_pending_outbound_connection(
                connection_id,
                maybe_peer,
                addresses,
                effective_role,
            )
        }

        fn handle_established_inbound_connection(
            &mut self,
            connection_id: ConnectionId,
            peer: PeerId,
            local_addr: &Multiaddr,
            remote_addr: &Multiaddr,
        ) -> Result<libp2p::swarm::THandler<Self>, ConnectionDenied> {
            self.inner.handle_established_inbound_connection(
                connection_id,
                peer,
                local_addr,
                remote_addr,
            )
        }

        fn handle_established_outbound_connection(
            &mut self,
            connection_id: ConnectionId,
            peer: PeerId,
            addr: &Multiaddr,
            role_override: Endpoint,
        ) -> Result<libp2p::swarm::THandler<Self>, ConnectionDenied> {
            self.inner.handle_established_outbound_connection(
                connection_id,
                peer,
                addr,
                role_override,
            )
        }

        fn on_swarm_event(&mut self, event: FromSwarm<'_>) {
            self.connection_tracker.handle_swarm_event(event);
            self.inner.on_swarm_event(event)
        }

        fn on_connection_handler_event(
            &mut self,
            peer_id: PeerId,
            connection_id: ConnectionId,
            event: THandlerOutEvent<Self>,
        ) {
            self.inner
                .on_connection_handler_event(peer_id, connection_id, event)
        }

        fn handle_pending_inbound_connection(
            &mut self,
            connection_id: ConnectionId,
            local_addr: &Multiaddr,
            remote_addr: &Multiaddr,
        ) -> Result<(), ConnectionDenied> {
            self.inner
                .handle_pending_inbound_connection(connection_id, local_addr, remote_addr)
        }
    }
}
