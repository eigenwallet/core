use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use bitcoin::hashes::{Hash, HashEngine, sha256};
use futures::FutureExt;
use libp2p::swarm::{
    ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, THandler, THandlerInEvent,
    THandlerOutEvent, ToSwarm,
};
use libp2p::{Multiaddr, PeerId, identity};
use safelog::DisplayRedacted;
use tokio::sync::mpsc;
use tor_hscrypto::pk::{HsId, HsIdKey, HsIdKeypair};
use tor_llcrypto::pk::ed25519;

use swap_machine::common::Database;

use super::ServiceRequest;

/// Configuration for the wormhole behaviour.
pub struct Config {
    /// How often to poll the database for new peers.
    pub poll_interval: Duration,
    /// Port for wormhole onion services.
    pub port: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(60),
            port: 9939,
        }
    }
}

pub struct Behaviour {
    /// The ASB's libp2p identity secret key bytes (32 bytes, ed25519).
    identity_secret: [u8; 32],
    /// Database for querying peers.
    db: Arc<dyn Database + Send + Sync>,
    /// Channel to send service spawn requests to the wrapper transport.
    service_tx: mpsc::UnboundedSender<ServiceRequest>,
    /// Map of wormhole onion address -> authorized peer.
    authorized_peers: HashMap<Multiaddr, PeerId>,
    /// Timer for periodic DB polling.
    poll_interval: tokio::time::Interval,
    /// Port for wormhole onion services.
    port: u16,
    /// Pending DB query result.
    pending_query: Option<tokio::task::JoinHandle<Vec<PeerId>>>,
}

impl Behaviour {
    pub fn new(
        identity: &identity::Keypair,
        db: Arc<dyn Database + Send + Sync>,
        service_tx: mpsc::UnboundedSender<ServiceRequest>,
        config: Config,
    ) -> Self {
        let ed25519_kp = identity
            .clone()
            .try_into_ed25519()
            .expect("ASB identity must be ed25519");
        let identity_secret: [u8; 32] = ed25519_kp.secret().as_ref().try_into().unwrap();

        let mut poll_interval = tokio::time::interval(config.poll_interval);
        // First tick fires immediately
        poll_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        Self {
            identity_secret,
            db,
            service_tx,
            authorized_peers: HashMap::new(),
            poll_interval,
            port: config.port,
            pending_query: None,
        }
    }

    /// Spawn a dedicated onion service for a peer.
    fn spawn_service_for_peer(&mut self, peer_id: PeerId) {
        if self.authorized_peers.values().any(|p| *p == peer_id) {
            return;
        }

        let keypair = derive_hs_keypair(&self.identity_secret, &peer_id);
        let multiaddr = keypair_to_multiaddr(&keypair, self.port);
        let nickname = derive_nickname(&peer_id);

        tracing::debug!(
            %peer_id,
            %multiaddr,
            %nickname,
            "Spawning wormhole onion service for peer"
        );

        self.authorized_peers.insert(multiaddr, peer_id);

        let _ = self
            .service_tx
            .send(ServiceRequest { keypair, nickname });
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = libp2p::swarm::dummy::ConnectionHandler;
    type ToSwarm = void::Void;

    fn handle_established_inbound_connection(
        &mut self,
        _: ConnectionId,
        peer_id: PeerId,
        local_addr: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        if let Some(authorized_peer) = self.authorized_peers.get(local_addr) {
            if peer_id != *authorized_peer {
                tracing::warn!(
                    %peer_id,
                    %local_addr,
                    "Rejecting connection to wormhole onion service from unauthorized peer"
                );
                return Err(ConnectionDenied::new(
                    "unauthorized peer for this wormhole onion service",
                ));
            }
        }
        Ok(libp2p::swarm::dummy::ConnectionHandler)
    }

    fn handle_established_outbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: &Multiaddr,
        _: libp2p::core::Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(libp2p::swarm::dummy::ConnectionHandler)
    }

    fn on_swarm_event(&mut self, _event: FromSwarm) {}

    fn on_connection_handler_event(
        &mut self,
        _: PeerId,
        _: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        void::unreachable(event)
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        // Check if a pending DB query has completed
        if let Some(handle) = &mut self.pending_query {
            if let Poll::Ready(result) = handle.poll_unpin(cx) {
                self.pending_query = None;
                match result {
                    Ok(trusted_peers) => {
                        for peer_id in trusted_peers {
                            self.spawn_service_for_peer(peer_id);
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = ?e, "Failed to query peers from database");
                    }
                }
            }
        }

        // Check if it's time to poll the DB
        if self.pending_query.is_none() && self.poll_interval.poll_tick(cx).is_ready() {
            let db = Arc::clone(&self.db);
            self.pending_query = Some(tokio::spawn(async move {
                match db.get_peers_with_swaps_past_btc_locked().await {
                    Ok(peers) => peers,
                    Err(e) => {
                        tracing::warn!(error = ?e, "Failed to query peers");
                        Vec::new()
                    }
                }
            }));

            cx.waker().wake_by_ref();
        }

        Poll::Pending
    }
}

/// Derive a deterministic HsIdKeypair for a dedicated onion service
/// for a specific peer.
fn derive_hs_keypair(identity_secret: &[u8; 32], peer_id: &PeerId) -> HsIdKeypair {
    let mut engine = sha256::HashEngine::default();
    engine.input(identity_secret);
    engine.input(b"WORMHOLE_ONION_SERVICE");
    engine.input(&peer_id.to_bytes());
    let hash = sha256::Hash::from_engine(engine);

    let keypair = ed25519::Keypair::from_bytes(&hash.to_byte_array());
    let expanded = ed25519::ExpandedKeypair::from(&keypair);

    HsIdKeypair::from(expanded)
}

/// Compute the onion Multiaddr that a given HsIdKeypair will produce.
fn keypair_to_multiaddr(keypair: &HsIdKeypair, port: u16) -> Multiaddr {
    let public: HsIdKey = keypair.into();
    let hs_id: HsId = public.into();
    let onion_domain = hs_id.display_unredacted().to_string();
    let onion_without_dot_onion = onion_domain
        .split('.')
        .nth(0)
        .expect("HsId display to contain .onion suffix");
    let multiaddr_string = format!("/onion3/{onion_without_dot_onion}:{port}");
    Multiaddr::from_str(&multiaddr_string).expect("valid onion multiaddr")
}

/// Derive a nickname for the wormhole onion service.
fn derive_nickname(peer_id: &PeerId) -> String {
    let peer_bytes = peer_id.to_bytes();
    let encoded = data_encoding::HEXLOWER.encode(&peer_bytes[..16.min(peer_bytes.len())]);
    format!("wh-{encoded}")
}
