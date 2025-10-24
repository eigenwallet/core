use crate::futures_util::FuturesHashSet;
use backoff::ExponentialBackoff;
use backoff::backoff::Backoff;
use libp2p::PeerId;
use libp2p::core::Multiaddr;
use libp2p::swarm::dial_opts::{DialOpts, PeerCondition};
use libp2p::swarm::{NetworkBehaviour, ToSwarm};
use std::collections::{HashMap, HashSet};
use std::future;
use std::task::{Context, Poll};
use std::time::Duration;
use void::Void;

/// A [`NetworkBehaviour`] that tracks whether we are connected to the given
/// peers and attempts to re-establish a connection with an exponential backoff
/// if we lose the connection.
pub struct Behaviour {
    /// The peers we are interested in.
    peers: HashSet<PeerId>,
    /// Tracks sleep timers for each peer waiting to redial.
    /// Futures in here yield the PeerId and when a Future completes we dial that peer
    sleep: FuturesHashSet<PeerId, ()>,
    /// Tracks the current backoff state for each peer.
    backoff: HashMap<PeerId, ExponentialBackoff>,
    /// Initial interval for backoff.
    initial_interval: Duration,
    /// Maximum interval for backoff.
    max_interval: Duration,
}

impl Behaviour {
    pub fn new(interval: Duration, max_interval: Duration) -> Self {
        Self {
            peers: HashSet::default(),
            sleep: FuturesHashSet::new(),
            backoff: HashMap::new(),
            initial_interval: interval,
            max_interval,
        }
    }

    /// Adds a peer to the set of peers to track. Returns true if the peer was newly added.
    pub fn add_peer(&mut self, peer: PeerId) -> bool {
        let newly_added = self.peers.insert(peer);
        if newly_added {
            self.sleep.insert(peer, Box::pin(std::future::ready(())));
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

    pub fn has_pending_redial(&self, peer: &PeerId) -> bool {
        self.sleep.contains_key(peer)
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = libp2p::swarm::dummy::ConnectionHandler;
    type ToSwarm = ();

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: libp2p::swarm::ConnectionId,
        peer: PeerId,
        _local_addr: &Multiaddr,
        _remote_addr: &Multiaddr,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        // Add the peer if it's not already tracked.
        self.add_peer(peer);

        // Reset the backoff state to start with the initial interval again once we disconnect again
        if let Some(backoff) = self.backoff.get_mut(&peer) {
            backoff.reset();
        }

        Ok(Self::ConnectionHandler {})
    }

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: libp2p::swarm::ConnectionId,
        peer: PeerId,
        _addr: &Multiaddr,
        _role_override: libp2p::core::Endpoint,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        // Add the peer if it's not already tracked.
        self.add_peer(peer);

        // Reset the backoff state to start with the initial interval again once we disconnect again
        if let Some(backoff) = self.backoff.get_mut(&peer) {
            backoff.reset();
        }

        Ok(Self::ConnectionHandler {})
    }

    fn on_swarm_event(&mut self, event: libp2p::swarm::FromSwarm<'_>) {
        let peer_to_redial = match event {
            libp2p::swarm::FromSwarm::ConnectionClosed(e) if self.peers.contains(&e.peer_id) => {
                Some(e.peer_id)
            }
            libp2p::swarm::FromSwarm::DialFailure(e) => match e.peer_id {
                Some(peer_id) if self.peers.contains(&peer_id) => Some(peer_id),
                _ => None,
            },
            _ => None,
        };

        if let Some(peer) = peer_to_redial {
            let backoff = self.get_backoff(&peer);

            let next_dial_in = match backoff.next_backoff() {
                Some(next_dial_in) => next_dial_in,
                None => {
                    unreachable!("The backoff should never run out of attempts");
                }
            };

            if self.sleep.insert(
                peer,
                Box::pin(async move {
                    tokio::time::sleep(next_dial_in).await;
                }),
            ) {
                tracing::info!(
                    peer_id = %peer,
                    seconds_until_next_redial = %next_dial_in.as_secs(),
                    "Waiting for next redial attempt"
                );
            }
        }
    }

    fn poll(&mut self, cx: &mut Context<'_>) -> std::task::Poll<ToSwarm<Self::ToSwarm, Void>> {
        // Check if any peer's sleep timer has completed
        // If it has, dial that peer
        match self.sleep.poll_next_unpin(cx) {
            Poll::Ready(Some((peer, _))) => Poll::Ready(ToSwarm::Dial {
                opts: DialOpts::peer_id(peer)
                    .condition(PeerCondition::Always)
                    .build(),
            }),
            Poll::Ready(None) | Poll::Pending => Poll::Pending,
        }
    }

    fn on_connection_handler_event(
        &mut self,
        _peer_id: PeerId,
        _connection_id: libp2p::swarm::ConnectionId,
        _event: libp2p::swarm::THandlerOutEvent<Self>,
    ) {
        unreachable!("The re-dial dummy connection handler does not produce any events");
    }
}
