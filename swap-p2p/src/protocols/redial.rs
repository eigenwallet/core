use crate::behaviour_util::{BackoffTracker, ConnectionTracker};
use crate::futures_util::FuturesHashSet;
use crate::out_event;
use libp2p::core::Multiaddr;
use libp2p::swarm::dial_opts::{DialOpts, PeerCondition};
use libp2p::swarm::{DialError, FromSwarm, NetworkBehaviour, ToSwarm};
use libp2p::PeerId;
use std::collections::{HashMap, HashSet, VecDeque};
use std::task::{Context, Poll};
use std::time::Duration;
use void::Void;

/// A [`NetworkBehaviour`] that tracks whether we are connected to the given
/// peers and attempts to re-establish a connection with an exponential backoff
/// if we lose the connection.
///
/// Note: Make sure that when using this as an inner behaviour for a `NetworkBehaviour` that you
/// call all the NetworkBehaviour methods (including `handle_pending_outbound_connection`) to ensure
/// that the addresses are cached correctly.
pub struct Behaviour {
    /// An identifier for this redial behaviour instance (for logging/tracing).
    name: &'static str,

    /// The peers we are interested in.
    peers: HashSet<PeerId>,

    connections: ConnectionTracker,

    /// Store address for all peers (even those we are not interested in)
    /// because we might be interested in them later on
    // TODO: Sort these by how often we were able to connect to them
    // TODO: Use the behaviour_util::AddressTracker instead
    addresses: HashMap<PeerId, HashSet<Multiaddr>>,

    /// Tracks sleep timers for each peer waiting to redial.
    /// Futures in here yield the PeerId and when a Future completes we dial that peer
    to_dial: FuturesHashSet<PeerId, ()>,

    /// Tracks the current backoff state for each peer.
    backoff: BackoffTracker,

    /// A queue of events to be sent to the swarm.
    to_swarm: VecDeque<ToSwarm<Event, Void>>,
}

impl Behaviour {
    pub fn new(name: &'static str, interval: Duration, max_interval: Duration) -> Self {
        Self {
            peers: HashSet::default(),
            addresses: HashMap::default(),
            to_dial: FuturesHashSet::new(),
            connections: ConnectionTracker::new(),
            backoff: BackoffTracker::new(
                interval,
                max_interval,
                crate::defaults::BACKOFF_MULTIPLIER,
            ),
            to_swarm: VecDeque::new(),
            name,
        }
    }

    /// Adds a peer to the set of peers to track. Returns true if the peer was newly added.
    #[tracing::instrument(level = "trace", name = "redial::add_peer", skip(self, peer), fields(redial_type = %self.name, peer = %peer))]
    pub fn add_peer(&mut self, peer: PeerId) -> bool {
        let newly_added = self.peers.insert(peer);

        // If the peer is newly added, schedule a dial immediately
        if newly_added {
            self.schedule_redial(&peer, Duration::ZERO);

            tracing::trace!("Started tracking peer");
        }

        newly_added
    }

    /// Removes a peer from the set of peers to track. Returns true if the peer was removed.
    #[tracing::instrument(level = "trace", name = "redial::remove_peer", skip(self, peer), fields(redial_type = %self.name, peer = %peer))]
    pub fn remove_peer(&mut self, peer: &PeerId) -> bool {
        if self.peers.remove(peer) {
            self.to_dial.remove(peer);

            tracing::trace!("Stopped tracking peer");
            return true;
        }

        false
    }

    /// Adds a peer to the set of peers to track with a specific address. Returns true if the peer was newly added.
    #[tracing::instrument(level = "trace", name = "redial::add_peer_with_address", skip(self, peer, address), fields(redial_type = %self.name, peer = %peer, address = %address))]
    pub fn add_peer_with_address(&mut self, peer: PeerId, address: Multiaddr) -> bool {
        let newly_added = self.peers.insert(peer);

        // If the peer is newly added, schedule a dial immediately
        if newly_added {
            self.schedule_redial(&peer, Duration::ZERO);

            tracing::trace!(
                ?address,
                "Started tracking peer and added a specific address"
            );
        }

        self.to_swarm.push_back(ToSwarm::NewExternalAddrOfPeer {
            peer_id: peer,
            address: address.clone(),
        });

        newly_added
    }

    #[tracing::instrument(level = "trace", name = "redial::schedule_redial", skip(self, peer, override_next_dial_in), fields(redial_type = %self.name, peer = %peer))]
    fn schedule_redial(
        &mut self,
        peer: &PeerId,
        override_next_dial_in: impl Into<Option<Duration>>,
    ) -> bool {
        // We first check if there already is a pending scheduled redial
        // because want do not want to increment the backoff if there is
        if self.to_dial.contains_key(peer) {
            return false;
        }

        // How long should we wait before we redial the peer?
        // If an override is provided, use that, otherwise use the backoff
        // TODO: Instead only increment on errors
        let next_dial_in = override_next_dial_in
            .into()
            .unwrap_or_else(|| self.backoff.increment(peer));

        let did_queue_new_dial = self.to_dial.insert(
            peer.clone(),
            Box::pin(async move {
                tokio::time::sleep(next_dial_in).await;
            }),
        );

        // We check if there is an entry before inserting a new one, so this should always be true
        // TODO: We could make this a production assert if we want to be more strict
        debug_assert!(did_queue_new_dial);

        self.to_swarm
            .push_back(ToSwarm::GenerateEvent(Event::ScheduledRedial {
                peer: peer.clone(),
                next_dial_in,
            }));

        tracing::trace!(
            seconds_until_next_redial = %next_dial_in.as_secs(),
            "Scheduled a redial attempt for a peer"
        );

        return true;
    }

    pub fn has_pending_redial(&self, peer: &PeerId) -> bool {
        self.to_dial.contains_key(peer)
    }

    pub fn insert_address(&mut self, peer: &PeerId, address: Multiaddr) -> bool {
        self.addresses
            .entry(peer.clone())
            .or_default()
            .insert(address)
    }
}

#[derive(Debug)]
pub enum Event {
    // TODO: This should emit useful events like Connected, Disconnected, etc. for the peers we are interested in.
    // This could prevent having to use the ConnectionTracker in parent behaviours (essentiually duplicating the code here)
    ScheduledRedial {
        peer: PeerId,
        next_dial_in: Duration,
    },
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = libp2p::swarm::dummy::ConnectionHandler;
    type ToSwarm = Event;

    #[tracing::instrument(level = "trace", name = "redial::on_swarm_event", skip(self, event), fields(redial_type = %self.name))]
    fn on_swarm_event(&mut self, event: FromSwarm<'_>) {
        self.connections.handle_swarm_event(event);

        let peer_to_redial = match event {
            // Check if we discovered a new address for some peer
            FromSwarm::NewExternalAddrOfPeer(event) => {
                // TOOD: Ensure that if the address contains a peer id it matches the peer id in the event
                if self.insert_address(&event.peer_id, event.addr.clone()) {
                    tracing::trace!(peer = %event.peer_id, address = %event.addr, "Cached an address for a peer");
                }

                None
            }
            // Check if the event was for either:
            // - a failed dial
            // - a closed connection
            //
            // We will then schedule a redial for the peer. We only do this if we are not already connected to the peer.
            FromSwarm::ConnectionClosed(event)
                if self.peers.contains(&event.peer_id)
                    && !self.connections.is_connected(&event.peer_id) =>
            {
                tracing::trace!(peer = %event.peer_id, "Connection closed. We will schedule a redial for this peer.");

                Some(event.peer_id)
            }
            FromSwarm::DialFailure(event) => match event.peer_id {
                Some(peer_id)
                    if self.peers.contains(&peer_id)
                        && !self.connections.is_connected(&peer_id) =>
                {
                    match event.error {
                        DialError::DialPeerConditionFalse(_) => {
                            // TODO: Can this lead to a condition where we will not redial the peer ever again? I don't think so...
                            //
                            // Reasoning:
                            // We always dial with `PeerCondition::DisconnectedAndNotDialing`.
                            // If we not disconnected, we don't need to redial.
                            // If we are already dialing, another event will be emitted if that dial fails.
                            // tracing::trace!(peer = %peer_id, dial_error = ?event.error, "A dial failure occurred for a peer we want to contineously redial, but this was due to a dial condition failure. We are not treating this as a failure. We will not schedule a redial.");
                            None
                        }
                        _ => {
                            tracing::trace!(peer = %peer_id, dial_error = ?event.error, "Dial failure occurred. We will schedule a redial for this peer.");
                            Some(peer_id)
                        }
                    }
                }
                _ => None,
            },
            _ => None,
        };

        // Check if the event was for a successful connection
        // We will then reset the backoff state for the peer
        let peer_to_reset = match event {
            FromSwarm::ConnectionEstablished(e) if self.peers.contains(&e.peer_id) => {
                Some(e.peer_id)
            }
            _ => None,
        };

        // Reset the backoff state for the peer if needed
        if let Some(peer) = peer_to_reset {
            self.backoff.reset(&peer);
        }

        // Schedule a redial if needed
        if let Some(peer) = peer_to_redial {
            self.schedule_redial(&peer, None);
        }
    }

    #[tracing::instrument(level = "trace", name = "redial::poll", skip(self, cx), fields(redial_type = %self.name))]
    fn poll(&mut self, cx: &mut Context<'_>) -> std::task::Poll<ToSwarm<Self::ToSwarm, Void>> {
        // Check if we have any event to send to the swarm
        if let Some(event) = self.to_swarm.pop_front() {
            return Poll::Ready(event);
        }

        // Check if any peer's sleep timer has completed
        // If it has, dial that peer
        if let Poll::Ready(Some((peer, _))) = self.to_dial.poll_next_unpin(cx) {
            return Poll::Ready(ToSwarm::Dial {
                opts: DialOpts::peer_id(peer)
                    .condition(PeerCondition::DisconnectedAndNotDialing)
                    .build(),
            });
        }

        Poll::Pending
    }

    fn on_connection_handler_event(
        &mut self,
        _peer_id: PeerId,
        _connection_id: libp2p::swarm::ConnectionId,
        _event: libp2p::swarm::THandlerOutEvent<Self>,
    ) {
        unreachable!("The re-dial dummy connection handler does not produce any events");
    }

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: libp2p::swarm::ConnectionId,
        _peer: PeerId,
        _local_addr: &Multiaddr,
        _remote_addr: &Multiaddr,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        Ok(Self::ConnectionHandler {})
    }

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: libp2p::swarm::ConnectionId,
        _peer: PeerId,
        _addr: &Multiaddr,
        _role_override: libp2p::core::Endpoint,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        Ok(Self::ConnectionHandler {})
    }

    #[tracing::instrument(level = "trace", name = "redial::handle_pending_outbound_connection", skip(self, connection_id, _addresses, maybe_peer, _effective_role), fields(redial_type = %self.name))]
    fn handle_pending_outbound_connection(
        &mut self,
        connection_id: libp2p::swarm::ConnectionId,
        maybe_peer: Option<PeerId>,
        _addresses: &[Multiaddr],
        _effective_role: libp2p::core::Endpoint,
    ) -> Result<Vec<Multiaddr>, libp2p::swarm::ConnectionDenied> {
        self.connections
            .handle_pending_outbound_connection(connection_id, maybe_peer);

        // If we don't know the peer id, we cannot contribute any addresses
        let Some(peer_id) = maybe_peer else {
            return Ok(vec![]);
        };

        // We only want to contribute addresses for peers we are instructed to redial
        if !self.peers.contains(&peer_id) {
            return Ok(vec![]);
        }

        // Cancel all pending dials for this peer in this behaviour
        // Another Behaviour already schedules a dial before we could
        if self.to_dial.remove(&peer_id) {
            tracing::trace!(peer = %peer_id, "Cancelled a pending dial for a peer because something else already scheduled a dial");
        }

        // Check if we have any addresses cached for the peer
        // TODO: Sort these by how often we were able to connect to them
        let addresses = self
            .addresses
            .get(&peer_id)
            .map(|addrs| addrs.iter().cloned().collect())
            .unwrap_or_default();

        // tracing::trace!(peer = %peer_id, contributed_addresses = ?addresses, "Contributing our cached addresses for a peer to the dial attempt");

        Ok(addresses)
    }
}

impl From<Event> for out_event::bob::OutEvent {
    fn from(event: Event) -> Self {
        out_event::bob::OutEvent::Redial(event)
    }
}

impl From<Event> for out_event::alice::OutEvent {
    fn from(_event: Event) -> Self {
        // TODO: Once this is used by Alice, convert this to a proper event
        out_event::alice::OutEvent::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::task::noop_waker;
    use libp2p::identity;
    use libp2p::swarm::{ConnectionId, DialFailure, ToSwarm};

    #[tokio::test]
    async fn add_peer_schedules_immediate_redial_event_and_another_dial_after_failure() {
        let mut behaviour =
            Behaviour::new("test", Duration::from_millis(10), Duration::from_secs(1));

        let peer = identity::Keypair::generate_ed25519().public().to_peer_id();

        // Add the peer
        let added = behaviour.add_peer(peer);
        assert!(added, "peer should be newly added");
        assert!(
            behaviour.has_pending_redial(&peer),
            "pending redial should be scheduled"
        );

        // Poll the behaviour directly until we see a Dial command targeting this peer
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);
        let deadline = tokio::time::Instant::now() + Duration::from_millis(50);

        // Wait for initial dial event
        loop {
            if tokio::time::Instant::now() >= deadline {
                panic!(
                    "behaviour did not emit Dial event for peer {} in time",
                    peer
                );
            }

            match behaviour.poll(&mut cx) {
                Poll::Ready(ToSwarm::Dial { opts }) => {
                    let dial_peer = opts
                        .get_peer_id()
                        .expect("dial opts should always contain a peer id");
                    assert_eq!(dial_peer, peer, "Dial should be for the correct peer");
                    break;
                }
                Poll::Ready(_) => {}
                Poll::Pending => {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            }
        }

        // Mock a dial failure
        behaviour.on_swarm_event(FromSwarm::DialFailure(DialFailure {
            peer_id: Some(peer),
            error: &DialError::Aborted,
            connection_id: ConnectionId::new_unchecked(0),
        }));

        loop {
            if tokio::time::Instant::now() >= deadline {
                panic!("behaviour did not emit Dial event for peer {} in time after a mocked dial failure", peer);
            }

            match behaviour.poll(&mut cx) {
                Poll::Ready(ToSwarm::Dial { opts }) => {
                    let dial_peer = opts
                        .get_peer_id()
                        .expect("dial opts should always contain a peer id");
                    assert_eq!(dial_peer, peer, "Dial should be for the correct peer");
                    break;
                }
                Poll::Ready(_) => {}
                Poll::Pending => {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            }
        }
    }
}
