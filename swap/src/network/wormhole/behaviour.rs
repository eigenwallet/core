use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use bitcoin::hashes::{Hash, HashEngine, sha256};
use futures::FutureExt;
use libp2p::request_response;
use libp2p::swarm::{
    ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, THandler, THandlerInEvent,
    THandlerOutEvent, ToSwarm,
};
use libp2p::{Multiaddr, PeerId, identity};
use safelog::DisplayRedacted;
use swap_p2p::behaviour_util::ConnectionTracker;
use swap_p2p::protocols::wormhole as proto;
use tokio::sync::mpsc;
use tor_hscrypto::pk::{HsId, HsIdKey, HsIdKeypair};
use tor_llcrypto::pk::ed25519;

use super::{PeerTrust, ServiceRequest};

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
    inner: proto::InnerBehaviour,
    connection_tracker: ConnectionTracker,
    /// Identity secret key bytes.
    /// We use this to derive the onion service keypair for a specific peer.
    identity_secret: [u8; 32],
    /// Provides trust information about peers.
    db: Arc<dyn PeerTrust + Send + Sync>,
    /// Channel to send service spawn requests to the wrapper transport.
    service_tx: mpsc::UnboundedSender<ServiceRequest>,
    /// Map of wormhole onion address -> authorized peer.
    authorized_peers: HashMap<Multiaddr, PeerId>,
    /// Peers we still need to push the wormhole address to.
    to_push: Vec<PeerId>,
    /// Timer for periodic polling of the trust provider.
    poll_interval: tokio::time::Interval,
    /// Port for wormhole onion services.
    port: u16,
    /// Pending trust provider query result.
    pending_query: Option<Pin<Box<dyn Future<Output = Vec<PeerId>> + Send>>>,
}

impl Behaviour {
    pub fn new(
        identity: &identity::Keypair,
        db: Arc<dyn PeerTrust + Send + Sync>,
        service_tx: mpsc::UnboundedSender<ServiceRequest>,
        config: Config,
    ) -> Self {
        let ed25519_kp = identity
            .clone()
            .try_into_ed25519()
            .expect("ASB identity must be ed25519");
        let identity_secret: [u8; 32] = ed25519_kp.secret().as_ref().try_into().unwrap();

        let mut poll_interval = tokio::time::interval(config.poll_interval);
        poll_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        Self {
            inner: proto::alice(),
            connection_tracker: ConnectionTracker::new(),
            identity_secret,
            db,
            service_tx,
            authorized_peers: HashMap::new(),
            to_push: Vec::new(),
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

        tracing::debug!(
            %peer_id,
            %multiaddr,
            "Spawning wormhole onion service for peer"
        );

        self.authorized_peers.insert(multiaddr, peer_id);

        let _ = self.service_tx.send(ServiceRequest { keypair, peer_id });

        // If peer is currently connected, push the updated (now active) address
        if self.connection_tracker.is_connected(&peer_id) {
            self.to_push.push(peer_id);
        }
    }

    fn is_active(&self, peer_id: &PeerId) -> bool {
        self.authorized_peers.values().any(|p| p == peer_id)
    }

    /// Push the wormhole address to a connected peer.
    fn push_to_peer(&mut self, peer_id: &PeerId) {
        let keypair = derive_hs_keypair(&self.identity_secret, peer_id);
        let address = keypair_to_multiaddr(&keypair, self.port);
        let active = self.is_active(peer_id);

        self.inner
            .send_request(peer_id, proto::Request { address, active });
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = <proto::InnerBehaviour as NetworkBehaviour>::ConnectionHandler;
    type ToSwarm = void::Void;

    fn handle_established_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer_id: PeerId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        // Reject unauthorized peers on wormhole addresses
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

        self.inner.handle_established_inbound_connection(
            connection_id,
            peer_id,
            local_addr,
            remote_addr,
        )
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer_id: PeerId,
        addr: &Multiaddr,
        role_override: libp2p::core::Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.inner.handle_established_outbound_connection(
            connection_id,
            peer_id,
            addr,
            role_override,
        )
    }

    fn handle_pending_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        maybe_peer: Option<PeerId>,
        addresses: &[Multiaddr],
        effective_role: libp2p::core::Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        self.connection_tracker
            .handle_pending_outbound_connection(connection_id, maybe_peer);

        self.inner.handle_pending_outbound_connection(
            connection_id,
            maybe_peer,
            addresses,
            effective_role,
        )
    }

    fn on_swarm_event(&mut self, event: FromSwarm<'_>) {
        self.connection_tracker.handle_swarm_event(event);

        if let FromSwarm::ConnectionEstablished(info) = &event {
            if !self.to_push.contains(&info.peer_id) {
                self.to_push.push(info.peer_id);
            }
        }

        self.inner.on_swarm_event(event);
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        self.inner
            .on_connection_handler_event(peer_id, connection_id, event);
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        // Push wormhole addresses to connected peers
        let mut i = 0;
        while i < self.to_push.len() {
            let peer_id = self.to_push[i];
            if self.connection_tracker.is_connected(&peer_id) {
                self.to_push.swap_remove(i);
                self.push_to_peer(&peer_id);
            } else {
                i += 1;
            }
        }

        // Drain inner events — we don't surface any of them
        while let Poll::Ready(event) = self.inner.poll(cx) {
            match event {
                ToSwarm::GenerateEvent(event) => {
                    if let request_response::Event::OutboundFailure { peer, error, .. } = event {
                        tracing::debug!(%peer, %error, "Failed to push wormhole address");
                    }
                }
                other => {
                    return Poll::Ready(other.map_out(|_| unreachable!()));
                }
            }
        }

        // Check if a pending DB query has completed
        if let Some(fut) = &mut self.pending_query {
            if let Poll::Ready(peers) = fut.poll_unpin(cx) {
                self.pending_query = None;
                for peer_id in peers {
                    self.spawn_service_for_peer(peer_id);
                }
            }
        }

        // Check if it's time to poll the DB
        if self.pending_query.is_none() && self.poll_interval.poll_tick(cx).is_ready() {
            let db = Arc::clone(&self.db);
            self.pending_query = Some(Box::pin(async move {
                match db.peers_with_financially_relevant_swap().await {
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
