pub mod alice;
pub mod bob;
pub mod transport;

use std::sync::Arc;

use anyhow::Result;
use libp2p::{Multiaddr, PeerId};
use tor_hscrypto::pk::HsIdKeypair;
use tor_hsservice::RunningOnionService;

/// Request sent from the behaviour to the wrapper transport to spawn a
/// dedicated onion service for a peer.
pub struct ServiceRequest {
    pub keypair: HsIdKeypair,
    pub peer_id: PeerId,
}

/// Sent from the transport back to the behaviour after a service is spawned.
pub struct ServiceHandle {
    pub peer_id: PeerId,
    pub service: Arc<RunningOnionService>,
}

/// Provides trust information about peers (Alice side).
#[async_trait::async_trait]
pub trait PeerTrust {
    /// Returns peers that have committed real funds to a swap.
    async fn peers_with_financially_relevant_swap(&self) -> Result<Vec<PeerId>>;
}

/// Stores wormhole addresses received from peers (Bob side).
#[async_trait::async_trait]
pub trait WormholeStore {
    async fn store_wormhole(&self, peer: PeerId, address: Multiaddr, active: bool) -> Result<()>;
    async fn get_wormhole(&self, peer: PeerId) -> Result<Option<(Multiaddr, bool)>>;
}
