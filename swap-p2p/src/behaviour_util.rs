use std::collections::{HashMap, HashSet};
use std::time::Duration;

use backoff::backoff::Backoff;
use backoff::ExponentialBackoff;
use libp2p::core::ConnectedPoint;
use libp2p::Multiaddr;
use libp2p::{
    swarm::{ConnectionId, FromSwarm},
    PeerId,
};

/// Used inside of a Behaviour to track connections to peers
pub struct ConnectionTracker {
    connections: HashMap<PeerId, HashSet<ConnectionId>>,
}

impl ConnectionTracker {
    pub fn new() -> Self {
        Self {
            connections: HashMap::new(),
        }
    }

    pub fn is_connected(&self, peer_id: &PeerId) -> bool {
        self.connections
            .get(peer_id)
            .map(|connections| !connections.is_empty())
            .unwrap_or(false)
    }

    pub fn handle_swarm_event(&mut self, event: FromSwarm<'_>) {
        match event {
            FromSwarm::ConnectionEstablished(connection_established) => {
                self.connections
                    .entry(connection_established.peer_id)
                    .or_insert_with(HashSet::new)
                    .insert(connection_established.connection_id);
            }
            FromSwarm::ConnectionClosed(connection_closed) => {
                self.connections
                    .entry(connection_closed.peer_id)
                    .and_modify(|connections| {
                        connections.remove(&connection_closed.connection_id);
                    });
            }
            _ => {}
        }
    }
}

/// Used inside of a Behaviour to track exponential backoff states for each peer.
pub struct BackoffTracker {
    backoffs: HashMap<PeerId, ExponentialBackoff>,
    initial_interval: Duration,
    max_interval: Duration,
    multiplier: f64,
}

impl BackoffTracker {
    pub fn new(initial: Duration, max: Duration, multiplier: f64) -> Self {
        Self {
            backoffs: HashMap::new(),
            initial_interval: initial,
            max_interval: max,
            multiplier,
        }
    }

    /// Get the backoff for a given peer.
    pub fn get(&mut self, peer: &PeerId) -> &mut ExponentialBackoff {
        self.backoffs
            .entry(*peer)
            .or_insert_with(|| ExponentialBackoff {
                initial_interval: self.initial_interval,
                current_interval: self.initial_interval,
                max_interval: self.max_interval,
                multiplier: self.multiplier,
                // Never give up
                max_elapsed_time: None,
                ..ExponentialBackoff::default()
            })
    }

    /// Reset the backoff state the given peer.
    pub fn reset(&mut self, peer: &PeerId) {
        if let Some(b) = self.backoffs.get_mut(peer) {
            b.reset();
        }
    }
}

/// Used inside of a Behaviour to track the last successful address for a peer
pub struct AddressTracker {
    addresses: HashMap<PeerId, Multiaddr>,
}

impl AddressTracker {
    pub fn new() -> Self {
        Self {
            addresses: HashMap::new(),
        }
    }

    pub fn handle_swarm_event(&mut self, event: FromSwarm<'_>) {
        match event {
            // If we connected as a dialer, record the address we connected to them at
            FromSwarm::ConnectionEstablished(connection_established) => {
                if let ConnectedPoint::Dialer { address, .. } = connection_established.endpoint {
                    self.addresses
                        .insert(connection_established.peer_id, address.clone());
                }
            }
            FromSwarm::NewExternalAddrOfPeer(new_external_addr_of_peer) => {
                // If we have never successfully connected to any address of the peer, we record the first announced address
                if !self
                    .addresses
                    .contains_key(&new_external_addr_of_peer.peer_id)
                {
                    self.addresses.insert(
                        new_external_addr_of_peer.peer_id,
                        new_external_addr_of_peer.addr.clone(),
                    );
                }
            }
            _ => (),
        }
    }

    pub fn last_seen_address(&self, peer_id: &PeerId) -> Option<Multiaddr> {
        self.addresses.get(peer_id).cloned()
    }
}
