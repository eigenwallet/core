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
// TODO: Track inflight dial attempts
pub struct ConnectionTracker {
    connections: HashMap<PeerId, HashSet<ConnectionId>>,
    inflight_dials: HashMap<PeerId, HashSet<ConnectionId>>,
}

impl ConnectionTracker {
    pub fn new() -> Self {
        Self {
            connections: HashMap::new(),
            inflight_dials: HashMap::new(),
        }
    }

    pub fn is_connected(&self, peer_id: &PeerId) -> bool {
        self.connections
            .get(peer_id)
            .map(|connections| !connections.is_empty())
            .unwrap_or(false)
    }

    pub fn has_inflight_dial(&self, peer_id: &PeerId) -> bool {
        self.inflight_dials
            .get(peer_id)
            .map(|dials| !dials.is_empty())
            .unwrap_or(false)
    }

    pub fn peers(&self) -> impl Iterator<Item = &PeerId> {
        self.connections.keys()
    }

    /// Any behaviour that uses the ConnectionTracker MUST call this method on every [`NetworkBehaviour::on_swarm_event`]
    ///
    /// Returns the peer id if the calling of this method resulted in a change of the internal state of that peer
    pub fn handle_swarm_event(&mut self, event: FromSwarm<'_>) -> Option<PeerId> {
        match event {
            FromSwarm::ConnectionEstablished(connection_established) => {
                self.connections
                    .entry(connection_established.peer_id)
                    .or_insert_with(HashSet::new)
                    .insert(connection_established.connection_id);

                // This dial attempts is no longer inflight because it has been established
                if let Some(inflight_dials) =
                    self.inflight_dials.get_mut(&connection_established.peer_id)
                {
                    if inflight_dials.remove(&connection_established.connection_id) {
                        return Some(connection_established.peer_id);
                    }
                }
            }
            FromSwarm::ConnectionClosed(connection_closed) => {
                self.connections
                    .entry(connection_closed.peer_id)
                    .and_modify(|connections| {
                        connections.remove(&connection_closed.connection_id);
                    });

                return Some(connection_closed.peer_id);
            }
            FromSwarm::DialFailure(dial_failure) => {
                // This dial attempts is no longer inflight because it has failed
                if let Some(peer_id) = dial_failure.peer_id {
                    if let Some(inflight_dials) = self.inflight_dials.get_mut(&peer_id) {
                        if inflight_dials.remove(&dial_failure.connection_id) {
                            return Some(peer_id);
                        }
                    }
                }
            }
            _ => {}
        }

        None
    }

    /// Any behaviour that uses the ConnectionTracker MUST call this method on every [`NetworkBehaviour::handle_pending_outbound_connection`]
    pub fn handle_pending_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        maybe_peer: Option<PeerId>,
    ) -> Option<PeerId> {
        if let Some(peer_id) = maybe_peer {
            if self
                .inflight_dials
                .entry(peer_id)
                .or_insert_with(HashSet::new)
                .insert(connection_id)
            {
                return Some(peer_id);
            }
        }

        None
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

    /// Increments the backoff for the given peer and returns the new backoff
    pub fn increment(&mut self, peer: &PeerId) -> Duration {
        self.get(peer)
            .next_backoff()
            .expect("backoff should never run out")
    }
}

/// Used inside of a Behaviour to track the last successful address for a peer
/// TODO: Track success/failure rates for each address
pub struct AddressTracker {
    addresses: HashMap<PeerId, Multiaddr>,
}

impl AddressTracker {
    pub fn new() -> Self {
        Self {
            addresses: HashMap::new(),
        }
    }

    /// Any behaviour that uses the AddressTracker MUST call this method on every [`NetworkBehaviour::on_swarm_event`]
    ///
    /// Returns the peer id if the calling of this method resulted in a change of the internal state of that peer
    pub fn handle_swarm_event(&mut self, event: FromSwarm<'_>) -> Option<PeerId> {
        match event {
            // If we connected as a dialer, record the address we connected to them at
            FromSwarm::ConnectionEstablished(connection_established) => {
                if let ConnectedPoint::Dialer { address, .. } = connection_established.endpoint {
                    let old_address = self
                        .addresses
                        .insert(connection_established.peer_id, address.clone());

                    // Return the peer id if the address was changed
                    if old_address.as_ref() != Some(address) {
                        return Some(connection_established.peer_id);
                    }
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

                    // Always return the peer id because the entry was previously empty
                    return Some(new_external_addr_of_peer.peer_id);
                }
            }
            _ => (),
        }

        None
    }

    pub fn peers(&self) -> impl Iterator<Item = &PeerId> {
        self.addresses.keys()
    }

    pub fn last_seen_address(&self, peer_id: &PeerId) -> Option<Multiaddr> {
        self.addresses.get(peer_id).cloned()
    }
}

/// Extracts the semver version from a user agent string.
/// Example input: "asb/2.0.0 (xmr-btc-swap-mainnet)"
/// Returns None if the version cannot be parsed.
pub fn extract_semver_from_agent_str(agent_str: &str) -> Option<semver::Version> {
    // Split on '/' and take the second part
    let version_str = agent_str.split('/').nth(1)?;
    // Split on whitespace and take the first part
    let version_str = version_str.split_whitespace().next()?;
    // Parse the version string
    semver::Version::parse(version_str).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p::core::{ConnectedPoint, Endpoint, Multiaddr};
    use libp2p::swarm::behaviour::{
        ConnectionClosed, ConnectionEstablished, DialFailure, NewExternalAddrOfPeer,
    };
    use libp2p::swarm::{ConnectionId, DialError, FromSwarm};
    use libp2p::PeerId;

    #[test]
    fn test_connection_tracker_basic() {
        let mut tracker = ConnectionTracker::new();
        let peer_id = PeerId::random();
        let conn_id = ConnectionId::new_unchecked(1);
        let endpoint = ConnectedPoint::Dialer {
            address: "/ip4/127.0.0.1/tcp/1234".parse().unwrap(),
            role_override: Endpoint::Dialer,
        };

        // Verify initially not connected
        assert!(!tracker.is_connected(&peer_id));

        // Simulate connection established
        let event = FromSwarm::ConnectionEstablished(ConnectionEstablished {
            peer_id,
            connection_id: conn_id,
            endpoint: &endpoint,
            failed_addresses: &[],
            other_established: 0,
        });

        tracker.handle_swarm_event(event);
        assert!(tracker.is_connected(&peer_id));

        // Simulate connection closed
        let event = FromSwarm::ConnectionClosed(ConnectionClosed {
            peer_id,
            connection_id: conn_id,
            endpoint: &endpoint,
            remaining_established: 0,
        });

        tracker.handle_swarm_event(event);
        assert!(!tracker.is_connected(&peer_id));
    }

    #[test]
    fn test_connection_tracker_inflight() {
        let mut tracker = ConnectionTracker::new();
        let peer_id = PeerId::random();
        let conn_id = ConnectionId::new_unchecked(1);

        // Add pending outbound
        tracker.handle_pending_outbound_connection(conn_id, Some(peer_id));
        assert!(tracker.has_inflight_dial(&peer_id));

        // Simulate dial failure
        let error = DialError::Aborted;
        let event = FromSwarm::DialFailure(DialFailure {
            peer_id: Some(peer_id),
            error: &error,
            connection_id: conn_id,
        });

        tracker.handle_swarm_event(event);
        assert!(!tracker.has_inflight_dial(&peer_id));
    }

    #[test]
    fn test_backoff_tracker() {
        let mut tracker = BackoffTracker::new(Duration::from_secs(1), Duration::from_secs(10), 2.0);
        let peer_id = PeerId::random();

        // Initial increment
        let backoff1 = tracker.increment(&peer_id);
        // With default randomization factor 0.5, it can be down to 0.5 * initial
        assert!(backoff1 >= Duration::from_millis(500));

        // Next increment increases
        let backoff2 = tracker.increment(&peer_id);
        assert!(backoff2 > backoff1);

        // Reset
        tracker.reset(&peer_id);

        // After reset, it should start over
        let backoff_after_reset = tracker.increment(&peer_id);
        assert!(backoff_after_reset < backoff2);
    }

    #[test]
    fn test_address_tracker() {
        let mut tracker = AddressTracker::new();
        let peer_id = PeerId::random();
        let addr: Multiaddr = "/ip4/127.0.0.1/tcp/1234".parse().unwrap();
        let endpoint = ConnectedPoint::Dialer {
            address: addr.clone(),
            role_override: Endpoint::Dialer,
        };
        let conn_id = ConnectionId::new_unchecked(1);

        // Connection established
        let event = FromSwarm::ConnectionEstablished(ConnectionEstablished {
            peer_id,
            connection_id: conn_id,
            endpoint: &endpoint,
            failed_addresses: &[],
            other_established: 0,
        });

        tracker.handle_swarm_event(event);

        assert_eq!(tracker.last_seen_address(&peer_id), Some(addr.clone()));
    }

    #[test]
    fn test_address_tracker_external_addr() {
        let mut tracker = AddressTracker::new();
        let peer_id = PeerId::random();
        let addr: Multiaddr = "/ip4/127.0.0.1/tcp/8080".parse().unwrap();

        let event = FromSwarm::NewExternalAddrOfPeer(NewExternalAddrOfPeer {
            peer_id,
            addr: &addr,
        });

        tracker.handle_swarm_event(event);
        assert_eq!(tracker.last_seen_address(&peer_id), Some(addr));
    }

    #[test]
    fn test_extract_semver() {
        let agent = "asb/2.0.0 (xmr-btc-swap-mainnet)";
        let version = extract_semver_from_agent_str(agent).unwrap();
        assert_eq!(version, semver::Version::new(2, 0, 0));

        let invalid = "invalid";
        assert!(extract_semver_from_agent_str(invalid).is_none());

        let agent_v3 = "asb/3.1.4-rc1 other-info";
        let version_v3 = extract_semver_from_agent_str(agent_v3).unwrap();
        assert_eq!(version_v3.major, 3);
        assert_eq!(version_v3.minor, 1);
        assert_eq!(version_v3.patch, 4);
    }
}
