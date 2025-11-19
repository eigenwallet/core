use backoff::{backoff::Backoff, ExponentialBackoff};
use libp2p::{
    core::Endpoint,
    identity,
    request_response::{self, OutboundFailure},
    swarm::{
        ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, THandlerInEvent,
        THandlerOutEvent, ToSwarm,
    },
    Multiaddr, PeerId, StreamProtocol,
};

use crate::{
    behaviour_util::ConnectionTracker,
    futures_util::FuturesHashSet,
    out_event,
    protocols::{
        notice,
        quote::{self, BidQuote},
    },
};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    task::Poll,
    time::Duration,
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

    /// Peers which have explictly told us that they do not support our protocol
    does_not_support: HashSet<PeerId>,

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
    pub fn new(
    ) -> Self {
        Self {
            inner: InnerBehaviour {
                quote: quote::bob(),
                notice: notice::Behaviour::new(StreamProtocol::new(quote::PROTOCOL)),
            },
            connection_tracker: ConnectionTracker::new(),
            does_not_support: HashSet::new(),
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
    type ConnectionHandler = <InnerBehaviour as libp2p::swarm::NetworkBehaviour>::ConnectionHandler;
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

        while let Poll::Ready(ready_to_swarm) = self.inner.poll(cx) {
            match ready_to_swarm {
                ToSwarm::GenerateEvent(event) => {
                    match event {
                        InnerBehaviourEvent::Notice(notice::Event::SupportsProtocol { peer }) => {
                            self.does_not_support.remove(&peer);
                        }
                        InnerBehaviourEvent::Quote(request_response::Event::Message {
                            peer,
                            message,
                        }) => {
                            if let request_response::Message::Response { response, .. } = message {
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
                        InnerBehaviourEvent::Quote(request_response::Event::OutboundFailure {
                            peer,
                            request_id,
                            error,
                        }) => {
                            // We got an outbound failure, so we increment the backoff
                            self.get_backoff(&peer)
                                .next_backoff()
                                .expect("backoff should never run out of attempts");

                            if let OutboundFailure::UnsupportedProtocols = error {
                                tracing::trace!(%peer, %request_id, %error, "Peer does not support the protocol, adding to does_not_support set");

                                self.does_not_support.insert(peer);

                            // Only schedule a new quote request if we are not sure that the peer does not support the protocol
                            } else if !self.does_not_support.contains(&peer) {
                                // We schedule a new quote request
                                let next_request_in =
                                    self.schedule_quote_request_with_backoff(peer);

                                tracing::trace!(%peer, %request_id, %error, next_request_in = %next_request_in.as_secs(), "Queuing quote request to peer after outbound failure");
                            }
                        }
                        other => {
                            tracing::trace!("quote::background::InnerBehaviourEvent: {:?}", other);
                        }
                    }
                }
                _ => {
                    return Poll::Ready(ready_to_swarm.map_out(|_| {
                        unreachable!("we handle all generate events in the arm above")
                    }));
                }
            }
        }

        while let Some(event) = self.to_swarm.pop_front() {
            return Poll::Ready(ToSwarm::GenerateEvent(event));
        }

        Poll::Pending
    }

    fn on_swarm_event(&mut self, event: FromSwarm<'_>) {
        self.connection_tracker.handle_swarm_event(event);

        match event {
            FromSwarm::ConnectionEstablished(connection_established) => {
                // When we connected to a peer where we are not certain that they do not support the protocol, we schedule a quote request
                if !self
                    .does_not_support
                    .contains(&connection_established.peer_id)
                {
                    self.schedule_quote_request_with_backoff(connection_established.peer_id);
                }
            }
            _ => {}
        }

        self.inner.on_swarm_event(event)
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
        self.inner
            .handle_established_outbound_connection(connection_id, peer, addr, role_override)
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

impl From<Event> for out_event::bob::OutEvent {
    fn from(event: Event) -> Self {
        match event {
            Event::QuoteReceived { peer, quote } => Self::BackgroundQuoteReceived { peer, quote },
        }
    }
}

impl From<Event> for out_event::alice::OutEvent {
    fn from(_: Event) -> Self {
        unreachable!("Alice should not use the quotes behaviour");
    }
}
