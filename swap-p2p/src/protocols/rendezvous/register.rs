use crate::behaviour_util::{AddressTracker, BackoffTracker, ConnectionTracker};
use crate::futures_util::FuturesHashSet;
use crate::protocols::redial;
use backoff::backoff::Backoff;
use futures::{future, FutureExt};
use libp2p::rendezvous::client::RegisterError;
use libp2p::rendezvous::ErrorCode;
use libp2p::swarm::{
    ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, THandler, THandlerInEvent,
    THandlerOutEvent, ToSwarm,
};
use libp2p::{identity, rendezvous, Multiaddr, PeerId};
use std::collections::{HashSet, VecDeque};
use std::task::{Context, Poll};
use std::time::Duration;

// TODO: This could use notice to recursively discover other rendezvous nodes
// TODO: Get the tests working again
pub struct Behaviour {
    inner: InnerBehaviour,

    rendezvous_nodes: Vec<PeerId>,
    namespace: rendezvous::Namespace,

    backoffs: BackoffTracker,
    connection: ConnectionTracker,
    address: AddressTracker,

    // Set of all peers that we think we are registered at
    registered: HashSet<PeerId>,

    // Register at these as soon as we are connected to them
    to_dispatch: VecDeque<PeerId>,

    // Move these into `to_dispatch` once the future resolves
    pending_to_dispatch: FuturesHashSet<PeerId, ()>,

    to_swarm: VecDeque<Event>,
}

#[derive(NetworkBehaviour)]
pub struct InnerBehaviour {
    rendezvous: rendezvous::client::Behaviour,
    redial: redial::Behaviour,
}

#[derive(Debug)]
pub enum Event {
    Registered {
        peer_id: PeerId,
    },
    RegisterDispatchFailed {
        peer_id: PeerId,
        error: RegisterError,
    },
    RegisterRequestFailed {
        peer_id: PeerId,
        error: ErrorCode,
    },
}

pub mod public {
    use libp2p::{Multiaddr, PeerId};

    #[derive(Debug, Clone)]
    pub struct RendezvousNodeStatus {
        pub peer_id: PeerId,
        pub address: Option<Multiaddr>,
        pub is_connected: bool,
        pub registration: RegisterStatus,
    }

    #[derive(Debug, Clone)]
    pub enum RegisterStatus {
        Registered,
        WillRegisterAfterDelay,
        RegisterOnceConnected,
        RequestInflight,
    }
}

impl Behaviour {
    const REDIAL_IDENTIFIER: &str = "rendezvous-register";

    pub fn new(
        identity: identity::Keypair,
        rendezvous_nodes: Vec<PeerId>,
        namespace: rendezvous::Namespace,
    ) -> Self {
        let backoffs = BackoffTracker::new(
            crate::defaults::RENDEZVOUS_RETRY_INITIAL_INTERVAL,
            crate::defaults::RENDEZVOUS_RETRY_MAX_INTERVAL,
            crate::defaults::BACKOFF_MULTIPLIER,
        );

        let mut redial = redial::Behaviour::new(
            Self::REDIAL_IDENTIFIER,
            crate::defaults::REDIAL_INITIAL_INTERVAL,
            crate::defaults::REDIAL_MAX_INTERVAL,
        );

        let mut pending_to_dispatch = FuturesHashSet::new();

        for &peer_id in &rendezvous_nodes {
            // We want to redial all of the nodes periodically because we only dispatch requests once we are connected
            redial.add_peer(peer_id.clone());

            // Schedule an intitial register
            pending_to_dispatch.insert(peer_id, Box::pin(future::ready(())));
        }

        Self {
            inner: InnerBehaviour {
                rendezvous: rendezvous::client::Behaviour::new(identity),
                redial,
            },
            connection: ConnectionTracker::new(),
            address: AddressTracker::new(),
            registered: HashSet::new(),
            rendezvous_nodes,
            backoffs,
            to_dispatch: VecDeque::new(),
            pending_to_dispatch,
            namespace,
            to_swarm: VecDeque::new(),
        }
    }

    pub fn status(&self) -> Vec<public::RendezvousNodeStatus> {
        self.rendezvous_nodes
            .iter()
            .map(|peer_id| {
                let registration = {
                    if self.registered.contains(peer_id) {
                        public::RegisterStatus::Registered
                    } else if self.to_dispatch.contains(peer_id) {
                        public::RegisterStatus::RegisterOnceConnected
                    } else {
                        public::RegisterStatus::WillRegisterAfterDelay
                    }
                };

                public::RendezvousNodeStatus {
                    peer_id: peer_id.clone(),
                    address: self.address.last_seen_address(peer_id),
                    is_connected: self.connection.is_connected(peer_id),
                    registration,
                }
            })
            .collect()
    }

    pub fn schedule_re_register_replace(&mut self, peer_id: PeerId) -> Duration {
        let backoff = self.backoffs.get(&peer_id).current_interval;

        // We replace any existing timeout
        self.pending_to_dispatch
            .replace(peer_id, Box::pin(tokio::time::sleep(backoff).boxed()));

        backoff
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = <InnerBehaviour as NetworkBehaviour>::ConnectionHandler;
    type ToSwarm = Event;

    fn on_swarm_event(&mut self, event: FromSwarm<'_>) {
        self.connection.handle_swarm_event(event);
        self.address.handle_swarm_event(event);
        self.inner.on_swarm_event(event);

        if let FromSwarm::ConnectionClosed(connection_closed) = event {
            // We disconnected from a node where we were registered, so we schedule a re-register
            if self.registered.contains(&connection_closed.peer_id) {
                let backoff = self.schedule_re_register_replace(connection_closed.peer_id);

                tracing::info!(
                    ?connection_closed.peer_id,
                    seconds_until_next_registration_attempt = %backoff.as_secs(),
                    "Disconnected from rendezvous node, scheduling re-register after backoff"
                );
            }
        }
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        // Check if any of the futures resolved
        while let Poll::Ready(Some((peer_id, _))) = self.pending_to_dispatch.poll_next_unpin(cx) {
            self.to_dispatch.push_back(peer_id);

            // We assume that if we have queued a register to be dispatched, then we are not registed anymore
            // because we only queue a register if we failed to register or the ttl expired
            self.registered.remove(&peer_id);
        }

        // Take ownership of the queue to avoid borrow checker issues
        let to_dispatch = std::mem::take(&mut self.to_dispatch);
        self.to_dispatch = to_dispatch
            .into_iter()
            .filter(|peer| {
                if !self.connection.is_connected(peer) {
                    return true;
                }

                // If we are connected to the peer, register with them
                if let Err(err) = self.inner.rendezvous.register(self.namespace.clone(), peer.clone(), None) {
                    // We failed to dispatch the register, so we backoff
                    self.backoffs.increment(&peer);

                    // Schedule a re-register
                    let backoff = self.schedule_re_register_replace(*peer);

                    tracing::error!(
                        ?peer,
                        ?err,
                        seconds_until_next_registration_attempt = %backoff.as_secs(),
                        "Failed to dispatch register at rendezvous node, scheduling retry after backoff"
                    );

                    // Inform swarm
                    self.to_swarm.push_back(Event::RegisterDispatchFailed { peer_id: peer.clone(), error: err });
                }

                false
            })
            .collect();

        // Reset the timer for the specific rendezvous node if we successfully registered
        while let Poll::Ready(event) = self.inner.poll(cx) {
            match event {
                ToSwarm::GenerateEvent(InnerBehaviourEvent::Rendezvous(
                    rendezvous::client::Event::Registered {
                        ttl,
                        rendezvous_node,
                        ..
                    },
                )) => {
                    // We successfully registered, so we backoff
                    self.backoffs.reset(&rendezvous_node);
                    self.registered.insert(rendezvous_node.clone());

                    // Schedule a re-registration after half of the TTL
                    let half_of_ttl = Duration::from_secs(ttl) / 2;
                    self.pending_to_dispatch.insert(
                        rendezvous_node.clone(),
                        tokio::time::sleep(half_of_ttl).boxed(),
                    );

                    // Inform swarm
                    self.to_swarm.push_back(Event::Registered {
                        peer_id: rendezvous_node,
                    });

                    tracing::info!(
                        ?rendezvous_node,
                        re_register_after_seconds = %half_of_ttl.as_secs(),
                        "Registered with rendezvous node"
                    );
                }
                ToSwarm::GenerateEvent(InnerBehaviourEvent::Rendezvous(
                    rendezvous::client::Event::RegisterFailed {
                        rendezvous_node,
                        namespace: _,
                        error,
                    },
                )) => {
                    // We failed to register, so we backoff
                    let backoff = self.backoffs.increment(&rendezvous_node);

                    // Inform swarm
                    self.to_swarm.push_back(Event::RegisterRequestFailed {
                        peer_id: rendezvous_node,
                        error,
                    });

                    tracing::error!(
                        ?rendezvous_node,
                        ?error,
                        seconds_until_next_registration_attempt = %backoff.as_secs(),
                        "Failed to register with rendezvous node, scheduling retry after backoff"
                    );

                    // Schedule a retry after the backoff
                    self.pending_to_dispatch
                        .insert(rendezvous_node.clone(), tokio::time::sleep(backoff).boxed());
                }
                ToSwarm::GenerateEvent(_) => {
                    // swallow all other generated events by the inner swarm
                    // TODO: Do something with these
                }
                other => {
                    return Poll::Ready(other.map_out(|_| {
                        unreachable!("we handled all generated events in the arm above")
                    }))
                }
            }
        }

        while let Some(event) = self.to_swarm.pop_front() {
            return Poll::Ready(ToSwarm::GenerateEvent(event));
        }

        Poll::Pending
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

    fn handle_established_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
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
        role_override: libp2p::core::Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.inner
            .handle_established_outbound_connection(connection_id, peer, addr, role_override)
    }

    fn handle_pending_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        maybe_peer: Option<PeerId>,
        addresses: &[Multiaddr],
        effective_role: libp2p::core::Endpoint,
    ) -> std::result::Result<Vec<Multiaddr>, ConnectionDenied> {
        self.inner.handle_pending_outbound_connection(
            connection_id,
            maybe_peer,
            addresses,
            effective_role,
        )
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

// TODO: Add a test that ensures we re-register after some time
#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocols::rendezvous::XmrBtcNamespace;
    use crate::test::{new_swarm, SwarmExt};
    use futures::StreamExt;
    use libp2p::rendezvous;
    use libp2p::swarm::SwarmEvent;

    #[tokio::test]
    async fn registers_once_at_two_rendezvous_nodes() {
        let (rendezvous_peer_id1, rendezvous_addr1, _) = spawn_rendezvous_server().await;
        let (rendezvous_peer_id2, rendezvous_addr2, _) = spawn_rendezvous_server().await;

        let mut asb = new_swarm(|identity| {
            super::Behaviour::new(
                identity,
                vec![rendezvous_peer_id1, rendezvous_peer_id2],
                XmrBtcNamespace::Testnet.into(),
            )
        });
        asb.add_peer_address(rendezvous_peer_id1, rendezvous_addr1);
        asb.add_peer_address(rendezvous_peer_id2, rendezvous_addr2);

        // We need to listen on address because otherwise we cannot advertise an address at the rendezvous point
        asb.listen_on_random_memory_address().await;

        let mut registered = HashSet::new();

        let asb_registered_three_times = tokio::spawn(async move {
            loop {
                if let SwarmEvent::Behaviour(Event::Registered { peer_id }) =
                    asb.select_next_some().await
                {
                    assert!(peer_id == rendezvous_peer_id1 || peer_id == rendezvous_peer_id2);
                    registered.insert(peer_id);
                }

                if registered.contains(&rendezvous_peer_id1)
                    && registered.contains(&rendezvous_peer_id2)
                {
                    break;
                }
            }
        });

        tokio::time::timeout(Duration::from_secs(5), asb_registered_three_times)
            .await
            .unwrap()
            .unwrap();
    }

    /// Spawns a rendezvous server that continuously processes events
    async fn spawn_rendezvous_server() -> (PeerId, Multiaddr, tokio::task::JoinHandle<()>) {
        let mut rendezvous_node = new_swarm(|_| {
            rendezvous::server::Behaviour::new(
                rendezvous::server::Config::default().with_min_ttl(2),
            )
        });
        let address = rendezvous_node.listen_on_random_memory_address().await;
        let peer_id = *rendezvous_node.local_peer_id();

        let handle = tokio::spawn(async move {
            loop {
                rendezvous_node.next().await;
            }
        });

        (peer_id, address, handle)
    }
}
