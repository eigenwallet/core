pub mod behaviour;
pub mod transport;

use tor_hscrypto::pk::HsIdKeypair;

/// Request sent from the behaviour to the wrapper transport to spawn a
/// dedicated onion service for a peer.
pub struct ServiceRequest {
    pub keypair: HsIdKeypair,
    pub nickname: String,
}
