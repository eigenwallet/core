use futures::future::{self};
use futures::FutureExt;
use libp2p::{
    identity, rendezvous,
    swarm::{NetworkBehaviour, THandlerInEvent, ToSwarm},
    Multiaddr, PeerId,
};
use std::{
    collections::{HashSet, VecDeque},
    task::Poll,
};

use crate::{
    behaviour_util::{BackoffTracker, ConnectionTracker, Trigger},
    futures_util::FuturesHashSet,
    protocols::redial,
};

pub struct Behaviour {
    inner: InnerBehaviour,
    namespace: rendezvous::Namespace,

    // Set of all (peer_id, address) pairs that we have discovered over the lifetime of the behaviour
    // This is also used to avoid notifying the swarm about the same pair multiple times
    discovered: HashSet<(PeerId, Multiaddr)>,

    // Track all the connections internally
    connection_tracker: ConnectionTracker,

    // Backoff for each rendezvous node for discovery
    backoff: BackoffTracker,

    // Queue of discovery requests to send to a rendezvous node
    // once the future resolves, the peer id is removed from the queue and a discovery request is sent
    pending_to_discover: FuturesHashSet<PeerId, ()>,

    // Queue of peers to send a request to as soon as we are connected to them
    to_discover: VecDeque<PeerId>,

    // Queue of events to be sent to the swarm
    to_swarm: VecDeque<ToSwarm<Event, THandlerInEvent<Self>>>,

    // The rendezvous nodes we are tracking
    // TODO: A bit redundant because it is stored in inner.redial
    rendezvous_nodes: HashSet<PeerId>,

    // Used to trigger an immediate refresh of discovery
    refresh: Trigger,
}

// This could use notice to recursively discover other rendezvous nodes
#[derive(NetworkBehaviour)]
pub struct InnerBehaviour {
    rendezvous: libp2p::rendezvous::client::Behaviour,
    redial: redial::Behaviour,
}

#[derive(Debug)]
pub enum Event {
    DiscoveredPeer { peer_id: PeerId },
}

impl Behaviour {
    pub fn new(
        identity: identity::Keypair,
        rendezvous_nodes: Vec<PeerId>,
        namespace: rendezvous::Namespace,
    ) -> Self {
        let mut redial = redial::Behaviour::new(
            "rendezvous-discovery",
            crate::defaults::REDIAL_INITIAL_INTERVAL,
            crate::defaults::REDIAL_MAX_INTERVAL,
        );
        let rendezvous = libp2p::rendezvous::client::Behaviour::new(identity);

        let backoff = BackoffTracker::new(
            crate::defaults::DISCOVERY_INITIAL_INTERVAL,
            crate::defaults::DISCOVERY_MAX_INTERVAL,
            crate::defaults::BACKOFF_MULTIPLIER,
        );
        let mut to_discover = FuturesHashSet::new();

        // Initialize backoff for each rendezvous node
        for node in &rendezvous_nodes {
            // We initially schedule a discovery request for each rendezvous node
            to_discover.insert(node.clone(), future::ready(()).boxed());

            // We instruct the redial behaviour to dial rendezvous nodes periodically
            redial.add_peer(node.clone());
        }

        Self {
            inner: InnerBehaviour { rendezvous, redial },
            discovered: HashSet::new(),
            backoff,
            pending_to_discover: to_discover,
            to_discover: VecDeque::new(),
            connection_tracker: ConnectionTracker::new(),
            namespace,
            to_swarm: VecDeque::new(),
            rendezvous_nodes: rendezvous_nodes.into_iter().collect(),
            refresh: Trigger::new(),
        }
    }

    /// Force an immediate refresh: clears all backoffs and schedules immediate
    /// discovery requests for all rendezvous nodes.
    pub fn force_refresh(&mut self) {
        self.refresh.trigger();
    }
}

impl NetworkBehaviour for Behaviour {
    // We use a dummy connection handler here as we don't need low level connection handling
    // This is handled by the inner behaviour
    type ConnectionHandler = <InnerBehaviour as NetworkBehaviour>::ConnectionHandler;

    type ToSwarm = Event;

    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<libp2p::swarm::ToSwarm<Self::ToSwarm, libp2p::swarm::THandlerInEvent<Self>>>
    {
        // Check if a refresh was requested
        if self.refresh.poll_next_unpin(cx).is_ready() {
            self.backoff.reset_all();
            self.pending_to_discover.clear();
            self.to_discover.clear();

            // Schedule immediate discovery for all rendezvous nodes
            for node in self.rendezvous_nodes.clone() {
                self.pending_to_discover
                    .insert(node, future::ready(()).boxed());
            }
        }

        // Check if a backoff timer for a peer has resolved
        while let Poll::Ready(Some((peer_id, _))) = self.pending_to_discover.poll_next_unpin(cx) {
            // Request is ready to be dispatched
            self.to_discover.push_back(peer_id);
        }

        // Take ownership of the queue to avoid borrow checker issues
        let to_discover = std::mem::take(&mut self.to_discover);
        self.to_discover = to_discover
            .into_iter()
            .filter(|peer| {
                // If we are not connected to the peer, keep it in the queue
                if !self.connection_tracker.is_connected(peer) {
                    return true;
                }

                // If we are connected to the peer, send a discovery request
                self.inner.rendezvous.discover(
                    Some(self.namespace.clone()),
                    None,
                    None,
                    peer.clone(),
                );

                false
            })
            .collect();

        while let Poll::Ready(event) = self.inner.poll(cx) {
            match event {
                ToSwarm::GenerateEvent(InnerBehaviourEvent::Rendezvous(
                    libp2p::rendezvous::client::Event::Discovered {
                        rendezvous_node,
                        registrations,
                        ..
                    },
                )) => {
                    tracing::trace!(
                        ?rendezvous_node,
                        num_registrations = %registrations.len(),
                        "Discovered peers at rendezvous node"
                    );

                    for registration in registrations {
                        for address in registration.record.addresses() {
                            let peer_id = registration.record.peer_id();

                            if self.discovered.insert((peer_id, address.clone())) {
                                self.to_swarm.push_back(ToSwarm::NewExternalAddrOfPeer {
                                    peer_id,
                                    address: address.clone(),
                                });
                                self.to_swarm.push_back(ToSwarm::GenerateEvent(
                                    Event::DiscoveredPeer { peer_id },
                                ));

                                tracing::trace!(
                                    ?rendezvous_node,
                                    ?peer_id,
                                    ?address,
                                    "Discovered peer at rendezvous node"
                                );
                            }

                            self.pending_to_discover.insert(
                                rendezvous_node,
                                tokio::time::sleep(crate::defaults::DISCOVERY_INTERVAL).boxed(),
                            );
                        }
                    }
                    continue;
                }
                ToSwarm::GenerateEvent(InnerBehaviourEvent::Rendezvous(
                    libp2p::rendezvous::client::Event::DiscoverFailed {
                        rendezvous_node,
                        error,
                        namespace: _,
                    },
                )) => {
                    let backoff = self.backoff.increment(&rendezvous_node);

                    self.pending_to_discover
                        .insert(rendezvous_node, tokio::time::sleep(backoff).boxed());

                    tracing::error!(
                        ?rendezvous_node,
                        ?error,
                        seconds_until_next_discovery_attempt = %backoff.as_secs(),
                        "Failed to discover peers at rendezvous node, scheduling retry after backoff"
                    );
                    continue;
                }
                ToSwarm::GenerateEvent(_) => {
                    // TODO: Do anything with these?
                }
                other => {
                    return Poll::Ready(other.map_out(|_| {
                        unreachable!("we handled all generated events in the arm above")
                    }))
                }
            }
        }

        // Check if we have any events to send to the swarm
        if let Some(event) = self.to_swarm.pop_front() {
            return Poll::Ready(event);
        }

        Poll::Pending
    }

    fn on_swarm_event(&mut self, event: libp2p::swarm::FromSwarm) {
        self.connection_tracker.handle_swarm_event(event);
        self.inner.on_swarm_event(event);
    }

    fn handle_established_inbound_connection(
        &mut self,
        connection_id: libp2p::swarm::ConnectionId,
        peer: PeerId,
        local_addr: &libp2p::Multiaddr,
        remote_addr: &libp2p::Multiaddr,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        self.inner.handle_established_inbound_connection(
            connection_id,
            peer,
            local_addr,
            remote_addr,
        )
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: libp2p::swarm::ConnectionId,
        peer: PeerId,
        addr: &libp2p::Multiaddr,
        role_override: libp2p::core::Endpoint,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        self.inner
            .handle_established_outbound_connection(connection_id, peer, addr, role_override)
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        connection_id: libp2p::swarm::ConnectionId,
        event: libp2p::swarm::THandlerOutEvent<Self>,
    ) {
        self.inner
            .on_connection_handler_event(peer_id, connection_id, event);
    }

    fn handle_pending_inbound_connection(
        &mut self,
        connection_id: libp2p::swarm::ConnectionId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<(), libp2p::swarm::ConnectionDenied> {
        self.inner
            .handle_pending_inbound_connection(connection_id, local_addr, remote_addr)
    }

    fn handle_pending_outbound_connection(
        &mut self,
        connection_id: libp2p::swarm::ConnectionId,
        maybe_peer: Option<PeerId>,
        addresses: &[Multiaddr],
        effective_role: libp2p::core::Endpoint,
    ) -> Result<Vec<Multiaddr>, libp2p::swarm::ConnectionDenied> {
        self.connection_tracker
            .handle_pending_outbound_connection(connection_id, maybe_peer);

        self.inner.handle_pending_outbound_connection(
            connection_id,
            maybe_peer,
            addresses,
            effective_role,
        )
    }
}
