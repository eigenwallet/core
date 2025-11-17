use std::collections::{HashMap, HashSet};
use std::time::Duration;

use backoff::backoff::Backoff;
use libp2p::{
    swarm::{ConnectionId, FromSwarm},
    PeerId,
};
use backoff::ExponentialBackoff;

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

/// Tracker for per-peer exponential backoff state.
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
    pub fn get_backoff(&mut self, peer: &PeerId) -> &mut ExponentialBackoff {
        self.backoffs.entry(*peer).or_insert_with(|| ExponentialBackoff {
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
