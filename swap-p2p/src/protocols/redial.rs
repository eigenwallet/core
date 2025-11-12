use crate::futures_util::FuturesHashSet;
use crate::out_event;
use backoff::backoff::Backoff;
use backoff::ExponentialBackoff;
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
/// TODO: Allow removing peers from the set after we are done with them.
pub struct Behaviour {
    /// The peers we are interested in.
    peers: HashSet<PeerId>,
    /// Store address for all peers (even those we are not interested in)
    /// because we might be interested in them later on
    // TODO: Sort these by how often we were able to connect to them
    addresses: HashMap<PeerId, HashSet<Multiaddr>>,
    /// Tracks sleep timers for each peer waiting to redial.
    /// Futures in here yield the PeerId and when a Future completes we dial that peer
    to_dial: FuturesHashSet<PeerId, ()>,
    /// Tracks the current backoff state for each peer.
    backoff: HashMap<PeerId, ExponentialBackoff>,
    /// Initial interval for backoff.
    initial_interval: Duration,
    /// Maximum interval for backoff.
    max_interval: Duration,
    /// A queue of events to be sent to the swarm.
    to_swarm: VecDeque<Event>,
    /// An identifier for this redial behaviour instance (for logging/tracing).
    name: &'static str,
}

impl Behaviour {
    pub fn new(name: &'static str, interval: Duration, max_interval: Duration) -> Self {
        Self {
            peers: HashSet::default(),
            addresses: HashMap::default(),
            to_dial: FuturesHashSet::new(),
            backoff: HashMap::new(),
            initial_interval: interval,
            max_interval,
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

            tracing::trace!("Added a new peer to the set of peers we want to contineously redial");
        }

        newly_added
    }

    fn get_backoff(&mut self, peer: &PeerId) -> &mut ExponentialBackoff {
        self.backoff.entry(*peer).or_insert_with(|| {
            ExponentialBackoff {
                initial_interval: self.initial_interval,
                current_interval: self.initial_interval,
                max_interval: self.max_interval,
                // We never give up on re-dialling
                max_elapsed_time: None,
                ..ExponentialBackoff::default()
            }
        })
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
        let next_dial_in = override_next_dial_in.into().unwrap_or_else(|| {
            self.get_backoff(peer)
                .next_backoff()
                .expect("redial backoff should never run out of attempts")
        });

        let did_queue_new_dial = self.to_dial.insert(
            peer.clone(),
            Box::pin(async move {
                tokio::time::sleep(next_dial_in).await;
            }),
        );

        // We check if there is an entry before inserting a new one, so this should always be true
        // TODO: We could make this a production assert if we want to be more strict
        debug_assert!(did_queue_new_dial);

        self.to_swarm.push_back(Event::ScheduledRedial {
            peer: peer.clone(),
            next_dial_in,
        });

        tracing::trace!(
            seconds_until_next_redial = %next_dial_in.as_secs(),
            "Scheduled a redial attempt for a peer"
        );

        return true;
    }

    pub fn has_pending_redial(&self, peer: &PeerId) -> bool {
        self.to_dial.contains_key(peer)
    }
}

#[derive(Debug)]
pub enum Event {
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
        let peer_to_redial = match event {
            // Check if we discovered a new address for some peer
            FromSwarm::NewExternalAddrOfPeer(event) => {
                // TOOD: Ensure that if the address contains a peer id it matches the peer id in the event
                self.addresses
                    .entry(event.peer_id)
                    .or_default()
                    .insert(event.addr.clone());

                tracing::trace!(peer = %event.peer_id, address = %event.addr, "Cached an address for a peer");

                None
            }
            // Check if the event was for either:
            // - a failed dial
            // - a closed connection
            //
            // We will then schedule a redial for the peer
            FromSwarm::ConnectionClosed(event) if self.peers.contains(&event.peer_id) => {
                tracing::trace!(peer = %event.peer_id, "A connection was closed for a peer we want to contineously redial. We will schedule a redial.");

                Some(event.peer_id)
            }
            FromSwarm::DialFailure(event) => match event.peer_id {
                Some(peer_id) if self.peers.contains(&peer_id) => {
                    match event.error {
                        DialError::DialPeerConditionFalse(_) => {
                            // TODO: Can this lead to a condition where we will not redial the peer ever again? I don't think so...
                            //
                            // Reasoning:
                            // We always dial with `PeerCondition::DisconnectedAndNotDialing`.
                            // If we not disconnected, we don't need to redial.
                            // If we are already dialing, another event will be emitted if that dial fails.
                            tracing::trace!(peer = %peer_id, dial_error = ?event.error, "A dial failure occurred for a peer we want to contineously redial, but this was due to a dial condition failure. We are not treating this as a failure. We will not schedule a redial.");
                            None
                        }
                        _ => {
                            tracing::trace!(peer = %peer_id, dial_error = ?event.error, "A dial failure occurred for a peer we want to contineously redial. We will schedule a redial.");
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
                tracing::trace!(peer = %e.peer_id, "A connection was established for a peer we want to contineously redial, resetting backoff state");

                Some(e.peer_id)
            }
            _ => None,
        };

        // Reset the backoff state for the peer if needed
        if let Some(peer) = peer_to_reset {
            if let Some(backoff) = self.backoff.get_mut(&peer) {
                backoff.reset();
            }
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
            return Poll::Ready(ToSwarm::GenerateEvent(event));
        }

        // Check if any peer's sleep timer has completed
        // If it has, dial that peer
        if let Poll::Ready(Some((peer, _))) = self.to_dial.poll_next_unpin(cx) {
            tracing::trace!(peer = %peer, "Instructing swarm to redial a peer we want to contineously redial after the sleep timer completed");

            // Actually dial the peer
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

    #[tracing::instrument(level = "trace", name = "redial::handle_pending_outbound_connection", skip(self, _connection_id, _addresses, _effective_role), fields(redial_type = %self.name))]
    fn handle_pending_outbound_connection(
        &mut self,
        _connection_id: libp2p::swarm::ConnectionId,
        maybe_peer: Option<PeerId>,
        _addresses: &[Multiaddr],
        _effective_role: libp2p::core::Endpoint,
    ) -> Result<Vec<Multiaddr>, libp2p::swarm::ConnectionDenied> {
        // If we don't know the peer id, we cannot contribute any addresses
        let Some(peer_id) = maybe_peer else {
            return Ok(vec![]);
        };

        // Check if we have any addresses cached for the peer
        // TODO: Sort these by how often we were able to connect to them
        let addresses = self
            .addresses
            .get(&peer_id)
            .map(|addrs| addrs.iter().cloned().collect())
            .unwrap_or_default();

        tracing::trace!(peer = %peer_id, addresses = ?addresses, "Contributing our cached addresses for a peer to the dial attempt");

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
