use backoff::backoff::Backoff;
use libp2p::{
    core::Endpoint,
    request_response::{self, OutboundFailure},
    swarm::{
        ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, THandlerInEvent,
        THandlerOutEvent, ToSwarm,
    },
    Multiaddr, PeerId, StreamProtocol,
};

use crate::{
    behaviour_util::{BackoffTracker, ConnectionTracker},
    futures_util::FuturesHashSet,
    protocols::{
        notice,
        quote::{self, BidQuote},
        redial,
    },
};
use std::{
    collections::{HashSet, VecDeque},
    task::Poll,
    time::Duration,
};

// We initially assume all peers support our protocol.
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
    backoff: BackoffTracker,

    // Queue of events to be sent to the swarm
    to_swarm: VecDeque<Event>,
}

impl Behaviour {
    pub fn new() -> Self {
        Self {
            inner: InnerBehaviour {
                quote: quote::bob(),
                notice: notice::Behaviour::new(StreamProtocol::new(quote::PROTOCOL)),
                redial: redial::Behaviour::new(
                    "quotes",
                    crate::defaults::QUOTE_REDIAL_INTERVAL,
                    crate::defaults::QUOTE_REDIAL_MAX_INTERVAL,
                ),
            },
            connection_tracker: ConnectionTracker::new(),
            does_not_support: HashSet::new(),
            to_dispatch: VecDeque::new(),
            to_request: FuturesHashSet::new(),
            backoff: BackoffTracker::new(
                crate::defaults::QUOTE_REDIAL_INTERVAL,
                crate::defaults::QUOTE_REDIAL_MAX_INTERVAL,
                crate::defaults::BACKOFF_MULTIPLIER,
            ),
            to_swarm: VecDeque::new(),
        }
    }

    fn is_connected(&self, peer_id: &PeerId) -> bool {
        self.connection_tracker.is_connected(peer_id)
    }

    fn schedule_quote_request_after(&mut self, peer: PeerId, duration: Duration) -> Duration {
        // TODO: Handle if there already was a future for this peer
        self.to_request
            .insert(peer, Box::pin(tokio::time::sleep(duration)));
        duration
    }

    fn schedule_quote_request_with_backoff(&mut self, peer: PeerId) -> Duration {
        let duration = self.backoff.get(&peer).current_interval;

        self.schedule_quote_request_after(peer, duration)
    }

    // Called whenever we hear about a new peer, this can be called multiple times for the same peer
    fn handle_discovered_peer(&mut self, peer: PeerId) {
        // Instruct to redial unless we know that the peer does not support the protocol
        if !self.does_not_support.contains(&peer) {
            self.inner.redial.add_peer(peer);
        }
    }

    fn handle_does_not_support_protocol(&mut self, peer: PeerId) {
        tracing::trace!(%peer, "Peer does not support the quote protocol");

        self.does_not_support.insert(peer);
        self.inner.redial.remove_peer(&peer);
        self.to_swarm
            .push_back(Event::DoesNotSupportProtocol { peer });
    }
}

#[derive(NetworkBehaviour)]
pub struct InnerBehaviour {
    quote: quote::Behaviour,
    notice: notice::Behaviour,
    redial: redial::Behaviour,
}

#[derive(Debug)]
pub enum Event {
    QuoteReceived { peer: PeerId, quote: BidQuote },
    DoesNotSupportProtocol { peer: PeerId },
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
                                self.backoff.reset(&peer);

                                // Schedule a new quote request after backoff
                                self.schedule_quote_request_after(
                                    peer,
                                    crate::defaults::QUOTE_INTERVAL,
                                );
                            }
                        }
                        InnerBehaviourEvent::Quote(request_response::Event::OutboundFailure {
                            peer,
                            request_id,
                            error,
                        }) => {
                            // We got an outbound failure, so we increment the backoff
                            self.backoff
                                .increment(&peer);

                            if let OutboundFailure::UnsupportedProtocols = error {
                                self.handle_does_not_support_protocol(peer);

                            // Only schedule a new quote request if we are not sure that the peer does not support the protocol
                            } else if !self.does_not_support.contains(&peer) {
                                // We schedule a new quote request
                                let next_request_in =
                                    self.schedule_quote_request_with_backoff(peer);

                                tracing::trace!(%peer, %request_id, %error, next_request_in = %next_request_in.as_secs(), "Queuing quote request to peer after outbound failure");
                            }
                        }
                        _other => {
                            // TODO: Do we need to handle other events?
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
            FromSwarm::NewExternalAddrOfPeer(event) => {
                self.handle_discovered_peer(event.peer_id);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::{new_swarm, SwarmExt};
    use futures::StreamExt;
    use libp2p::swarm::{Swarm, SwarmEvent};
    use tokio::task::JoinHandle;

    #[tokio::test]
    async fn receive_quote_from_alice() {
        // Create the swarm for Bob
        let mut bob = new_swarm(|_| Behaviour::new());

        // Create the swarm for Alice
        // Let her listen on a random memory address
        // Let her respond to requests
        let alice = new_swarm(|_| quote::alice());
        let (alice_peer_id, alice_addr, alice_handle) = serve_quotes(alice).await;

        // Tell Bob about Alice's address
        // This should be enough to get the `quotes` behaviour to dial Alice
        bob.add_peer_address(alice_peer_id, alice_addr);

        let bob_handle = tokio::spawn(async move {
            loop {
                if let SwarmEvent::Behaviour(Event::QuoteReceived { peer, quote }) =
                    bob.select_next_some().await
                {
                    if peer == alice_peer_id && quote == quote::BidQuote::ZERO {
                        return;
                    }
                }
            }
        });

        tokio::select! {
            _ = bob_handle => {}
            _ = tokio::time::sleep(Duration::from_secs(10)) => {
                panic!("Test timed out");
            }
        }

        alice_handle.abort();
    }

    #[tokio::test]
    async fn receive_does_not_support_protocol_from_alice() {
        // Create the swarm for Bob
        let mut bob = new_swarm(|_| Behaviour::new());

        // Use quote::bob() so Alice doesn't support inbound requests
        let mut alice = new_swarm(|_| quote::bob());
        let alice_peer_id = *alice.local_peer_id();
        let alice_addr = alice.listen_on_random_memory_address().await;

        // Let Alice run but she doesn't need to do anything
        let alice_handle = tokio::spawn(async move {
            loop {
                alice.select_next_some().await;
            }
        });

        // Tell Bob about Alice's address
        bob.add_peer_address(alice_peer_id, alice_addr);

        // Wait for DoesNotSupportProtocol
        let timeout = tokio::time::sleep(Duration::from_secs(5));
        tokio::pin!(timeout);

        loop {
            tokio::select! {
                event = bob.select_next_some() => {
                    if let SwarmEvent::Behaviour(Event::DoesNotSupportProtocol { peer }) = event {
                        if peer == alice_peer_id {
                            break;
                        }
                    }
                }
                _ = &mut timeout => {
                    panic!("Timeout waiting for DoesNotSupportProtocol");
                }
            }
        }

        // Kill Alice which should kill the connection
        // Bob should not attempt to redial Alice because he has noticed that she does not support the protocol
        alice_handle.abort();

        let mut connection_closed = false;
        let timeout = tokio::time::sleep(Duration::from_secs(5));
        tokio::pin!(timeout);

        loop {
            tokio::select! {
                event = bob.select_next_some() => {
                     match event {
                        SwarmEvent::ConnectionClosed { peer_id, .. } if peer_id == alice_peer_id => {
                            connection_closed = true;
                        }
                        SwarmEvent::OutgoingConnectionError { peer_id: Some(peer_id), .. } if peer_id == alice_peer_id => {
                            panic!("Bob attempted to redial Alice!");
                        }
                        _ => {}
                     }
                }
                _ = &mut timeout => {
                    break;
                }
            }
        }

        assert!(
            connection_closed,
            "Bob should have noticed connection closed"
        );
    }

    /// Ensures that Alice responds with a zero quote when requested.
    async fn serve_quotes(
        mut alice: Swarm<quote::Behaviour>,
    ) -> (PeerId, Multiaddr, JoinHandle<()>) {
        let alice_peer_id = *alice.local_peer_id();
        let alice_addr = alice.listen_on_random_memory_address().await;

        let alice_handle = tokio::spawn(async move {
            loop {
                match alice.select_next_some().await {
                    SwarmEvent::Behaviour(libp2p::request_response::Event::Message {
                        message: libp2p::request_response::Message::Request { channel, .. },
                        ..
                    }) => {
                        alice
                            .behaviour_mut()
                            .send_response(channel, quote::BidQuote::ZERO)
                            .unwrap();
                    }
                    _ => {}
                }
            }
        });

        (alice_peer_id, alice_addr, alice_handle)
    }
}
