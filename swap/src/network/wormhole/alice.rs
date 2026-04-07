use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use bitcoin::hashes::{Hash, HashEngine, sha256};
use futures::FutureExt;
use libp2p::request_response::{self, OutboundRequestId};
use libp2p::swarm::{
    ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, THandler, THandlerInEvent,
    THandlerOutEvent, ToSwarm,
};
use libp2p::{Multiaddr, PeerId, identity};
use safelog::DisplayRedacted;
use swap_p2p::behaviour_util::ConnectionTracker;
use swap_p2p::futures_util::FuturesHashSet;
use swap_p2p::protocols::wormhole as proto;
use tokio::sync::mpsc;
use tor_hscrypto::pk::{HsId, HsIdKey, HsIdKeypair};
use tor_hsservice::RunningOnionService;
use tor_llcrypto::pk::ed25519;

use super::{PeerTrust, ServiceHandle, ServiceRequest};

const RETRY_INITIAL: Duration = Duration::from_secs(5);
const RETRY_MAX: Duration = Duration::from_secs(60);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// Configuration for the wormhole behaviour.
pub struct Config {
    /// How often to poll the trust provider for new peers.
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

/// What we last successfully pushed to a peer.
#[derive(Clone, PartialEq, Eq)]
struct SentState {
    address: Multiaddr,
    active: bool,
}

/// Status snapshot of a single wormhole onion service.
#[derive(Debug)]
pub struct WormholeServiceInfo {
    pub peer_id: PeerId,
    pub address: Multiaddr,
    pub status: String,
}

pub struct Behaviour {
    inner: proto::InnerBehaviour,
    connection_tracker: ConnectionTracker,
    /// Identity secret key bytes.
    identity_secret: [u8; 32],
    /// Provides trust information about peers.
    trust_provider: Arc<dyn PeerTrust + Send + Sync>,
    /// Channel to send service spawn requests to the wrapper transport.
    service_tx: mpsc::UnboundedSender<ServiceRequest>,
    /// Map of wormhole onion address -> authorized peer.
    authorized_peers: HashMap<Multiaddr, PeerId>,
    /// Running onion service handles for status queries.
    service_handles: HashMap<PeerId, Arc<RunningOnionService>>,
    /// Receives service handles back from the transport after spawning.
    handle_rx: mpsc::UnboundedReceiver<ServiceHandle>,
    /// Peers waiting for their timer to expire before dispatch.
    pending: FuturesHashSet<PeerId, ()>,
    /// Peers ready to dispatch (timer elapsed, waiting for connection).
    to_dispatch: VecDeque<PeerId>,
    /// Inflight pushes: request_id -> peer.
    inflight: HashMap<OutboundRequestId, PeerId>,
    /// What we last successfully pushed to each peer.
    last_sent: HashMap<PeerId, SentState>,
    /// Current retry delay per peer (doubles on failure, resets on success).
    retry_delay: HashMap<PeerId, Duration>,
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
        trust_provider: Arc<dyn PeerTrust + Send + Sync>,
        service_tx: mpsc::UnboundedSender<ServiceRequest>,
        handle_rx: mpsc::UnboundedReceiver<ServiceHandle>,
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
            trust_provider,
            service_tx,
            authorized_peers: HashMap::new(),
            service_handles: HashMap::new(),
            handle_rx,
            pending: FuturesHashSet::new(),
            to_dispatch: VecDeque::new(),
            inflight: HashMap::new(),
            last_sent: HashMap::new(),
            retry_delay: HashMap::new(),
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

        // The active flag changed — schedule a push
        self.schedule_push(peer_id);
    }

    /// Returns a snapshot of all wormhole services and their current status.
    pub fn services(&self) -> Vec<WormholeServiceInfo> {
        self.authorized_peers
            .iter()
            .map(|(addr, peer)| {
                let status = self
                    .service_handles
                    .get(peer)
                    .map(|svc| format!("{:?}", svc.status().state()))
                    .unwrap_or_else(|| "starting".to_string());
                WormholeServiceInfo {
                    peer_id: *peer,
                    address: addr.clone(),
                    status,
                }
            })
            .collect()
    }

    fn is_active(&self, peer_id: &PeerId) -> bool {
        self.authorized_peers.values().any(|p| p == peer_id)
    }

    /// Compute what we would send to this peer right now.
    fn current_state_for(&self, peer_id: &PeerId) -> SentState {
        let keypair = derive_hs_keypair(&self.identity_secret, peer_id);
        let address = keypair_to_multiaddr(&keypair, self.port);
        let active = self.is_active(peer_id);
        SentState { address, active }
    }

    /// Schedule a push to a peer using the current retry delay.
    /// Skips if the state hasn't changed since last successful send.
    fn schedule_push(&mut self, peer_id: PeerId) {
        let current = self.current_state_for(&peer_id);
        if self.last_sent.get(&peer_id) == Some(&current) {
            return;
        }
        let delay = self
            .retry_delay
            .get(&peer_id)
            .copied()
            .unwrap_or(RETRY_INITIAL);
        self.pending
            .replace(peer_id, Box::pin(tokio::time::sleep(delay)));
    }

    /// Schedule a push after the heartbeat interval, bypassing the last_sent check.
    fn schedule_heartbeat(&mut self, peer_id: PeerId) {
        self.pending
            .replace(peer_id, Box::pin(tokio::time::sleep(HEARTBEAT_INTERVAL)));
    }

    /// Actually send the push to a connected peer.
    fn dispatch_push(&mut self, peer_id: &PeerId) {
        let state = self.current_state_for(peer_id);
        let request_id = self.inner.send_request(
            peer_id,
            proto::Request {
                address: state.address,
                active: state.active,
            },
        );
        self.inflight.insert(request_id, *peer_id);
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
            self.schedule_push(info.peer_id);
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
        // Move peers whose timer has expired into the dispatch queue
        while let Poll::Ready(Some((peer_id, ()))) = self.pending.poll_next_unpin(cx) {
            self.to_dispatch.push_back(peer_id);
        }

        // Dispatch to connected peers, keep non-connected ones in queue
        let to_dispatch = std::mem::take(&mut self.to_dispatch);
        self.to_dispatch = to_dispatch
            .into_iter()
            .filter(|peer_id| {
                if self.connection_tracker.is_connected(peer_id) {
                    self.dispatch_push(peer_id);
                    false
                } else {
                    true
                }
            })
            .collect();

        // Drain inner events
        while let Poll::Ready(event) = self.inner.poll(cx) {
            match event {
                ToSwarm::GenerateEvent(event) => match event {
                    request_response::Event::Message {
                        message: request_response::Message::Response { request_id, .. },
                        peer,
                        ..
                    } => {
                        self.inflight.remove(&request_id);
                        self.retry_delay.remove(&peer);

                        let state = self.current_state_for(&peer);
                        self.last_sent.insert(peer, state);

                        self.schedule_heartbeat(peer);
                    }
                    request_response::Event::OutboundFailure {
                        peer,
                        request_id,
                        error,
                    } => {
                        self.inflight.remove(&request_id);

                        // Double the retry delay, capped at RETRY_MAX
                        let current = self
                            .retry_delay
                            .get(&peer)
                            .copied()
                            .unwrap_or(RETRY_INITIAL);
                        self.retry_delay.insert(peer, (current * 2).min(RETRY_MAX));

                        tracing::debug!(%peer, %error, "Failed to push wormhole address, will retry");

                        self.schedule_push(peer);
                    }
                    _ => {}
                },
                other => {
                    return Poll::Ready(other.map_out(|_| unreachable!()));
                }
            }
        }

        // Drain service handles sent back from the transport
        while let Poll::Ready(Some(handle)) = self.handle_rx.poll_recv(cx) {
            self.service_handles.insert(handle.peer_id, handle.service);
        }

        // Check if a pending trust provider query has completed
        if let Some(fut) = &mut self.pending_query {
            if let Poll::Ready(peers) = fut.poll_unpin(cx) {
                self.pending_query = None;
                for peer_id in peers {
                    self.spawn_service_for_peer(peer_id);
                }
            }
        }

        // Check if it's time to poll the trust provider
        if self.pending_query.is_none() && self.poll_interval.poll_tick(cx).is_ready() {
            let trust_provider = Arc::clone(&self.trust_provider);
            self.pending_query = Some(Box::pin(async move {
                match trust_provider.peers_with_financially_relevant_swap().await {
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
