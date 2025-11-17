use std::collections::{HashMap, HashSet};

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
