// Copyright 2023 Protocol Labs.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

use libp2p_core::{ConnectedPoint, Endpoint, Multiaddr};
use libp2p_identity::PeerId;
use libp2p_swarm::{
    behaviour::{ConnectionEstablished, DialFailure, ListenFailure},
    dummy, ConnectionClosed, ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, THandler,
    THandlerInEvent, THandlerOutEvent, ToSwarm,
};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use void::Void;

use futures::FutureExt;
use futures::future::{BoxFuture, Fuse, FusedFuture, OptionFuture};

use crate::network::wormhole::PeerTrust;

/// A [`NetworkBehaviour`] that enforces a set of [`ConnectionLimits`].
///
/// For these limits to take effect, this needs to be composed into the behaviour tree of your application.
///
/// If a connection is denied due to a limit, either a [`SwarmEvent::IncomingConnectionError`](libp2p_swarm::SwarmEvent::IncomingConnectionError)
/// or [`SwarmEvent::OutgoingConnectionError`](libp2p_swarm::SwarmEvent::OutgoingConnectionError) will be emitted.
/// The [`ListenError::Denied`](libp2p_swarm::ListenError::Denied) and respectively the [`DialError::Denied`](libp2p_swarm::DialError::Denied) variant
/// contain a [`ConnectionDenied`] type that can be downcast to [`Exceeded`] error if (and only if) **this**
/// behaviour denied the connection.
///
/// If you employ multiple [`NetworkBehaviour`]s that manage connections, it may also be a different error.
///
/// # Example
///
/// ```rust
/// # use libp2p_identify as identify;
/// # use libp2p_ping as ping;
/// # use libp2p_swarm_derive::NetworkBehaviour;
/// # use libp2p_connection_limits as connection_limits;
///
/// #[derive(NetworkBehaviour)]
/// # #[behaviour(prelude = "libp2p_swarm::derive_prelude")]
/// struct MyBehaviour {
///   identify: identify::Behaviour,
///   ping: ping::Behaviour,
///   limits: connection_limits::Behaviour
/// }
/// ```
pub struct Behaviour {
    limits: ConnectionLimits,

    pending_inbound_connections: HashSet<ConnectionId>,
    pending_outbound_connections: HashSet<ConnectionId>,
    established_inbound_connections: HashSet<ConnectionId>,
    established_outbound_connections: HashSet<ConnectionId>,
    established_per_peer: HashMap<PeerId, HashSet<ConnectionId>>,

    honest_peers: HashSet<PeerId>,
    trust_provider: Arc<dyn PeerTrust + Send + Sync>,
    freshness_days: u64,
    max_honest_peers: usize,
    poll_interval: tokio::time::Interval,
    pending_query: OptionFuture<Fuse<BoxFuture<'static, Option<Vec<PeerId>>>>>,
}

impl Behaviour {
    pub fn new(
        limits: ConnectionLimits,
        trust_provider: Arc<dyn PeerTrust + Send + Sync>,
        freshness_days: u64,
        max_honest_peers: usize,
        poll_interval: Duration,
    ) -> Self {
        let mut poll_interval = tokio::time::interval(poll_interval);
        poll_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        Self {
            limits,
            pending_inbound_connections: Default::default(),
            pending_outbound_connections: Default::default(),
            established_inbound_connections: Default::default(),
            established_outbound_connections: Default::default(),
            established_per_peer: Default::default(),
            honest_peers: Default::default(),
            trust_provider,
            freshness_days,
            max_honest_peers,
            poll_interval,
            pending_query: OptionFuture::from(None),
        }
    }

    /// Returns a mutable reference to [`ConnectionLimits`].
    /// > **Note**: A new limit will not be enforced against existing connections.
    pub fn limits_mut(&mut self) -> &mut ConnectionLimits {
        &mut self.limits
    }
}

fn check_limit(limit: Option<u32>, current: usize, kind: Kind) -> Result<(), ConnectionDenied> {
    let limit = limit.unwrap_or(u32::MAX);
    let current = current as u32;

    if current >= limit {
        return Err(ConnectionDenied::new(Exceeded { limit, kind }));
    }

    Ok(())
}

/// A connection limit has been exceeded.
#[derive(Debug, Clone, Copy)]
pub struct Exceeded {
    limit: u32,
    kind: Kind,
}

impl Exceeded {
    pub fn limit(&self) -> u32 {
        self.limit
    }
}

impl fmt::Display for Exceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "connection limit exceeded: at most {} {} are allowed",
            self.limit, self.kind
        )
    }
}

#[derive(Debug, Clone, Copy)]
enum Kind {
    PendingIncoming,
    PendingOutgoing,
    EstablishedIncoming,
    EstablishedOutgoing,
    EstablishedPerPeer,
    EstablishedTotal,
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Kind::PendingIncoming => write!(f, "pending incoming connections"),
            Kind::PendingOutgoing => write!(f, "pending outgoing connections"),
            Kind::EstablishedIncoming => write!(f, "established incoming connections"),
            Kind::EstablishedOutgoing => write!(f, "established outgoing connections"),
            Kind::EstablishedPerPeer => write!(f, "established connections per peer"),
            Kind::EstablishedTotal => write!(f, "established connections"),
        }
    }
}

impl std::error::Error for Exceeded {}

/// The configurable connection limits.
#[derive(Debug, Clone, Default)]
pub struct ConnectionLimits {
    max_pending_incoming: Option<u32>,
    max_pending_outgoing: Option<u32>,
    max_established_incoming: Option<u32>,
    max_established_outgoing: Option<u32>,
    max_established_per_peer: Option<u32>,
    max_established_total: Option<u32>,
}

impl ConnectionLimits {
    /// Configures the maximum number of concurrently incoming connections being established.
    pub fn with_max_pending_incoming(mut self, limit: Option<u32>) -> Self {
        self.max_pending_incoming = limit;
        self
    }

    /// Configures the maximum number of concurrently outgoing connections being established.
    pub fn with_max_pending_outgoing(mut self, limit: Option<u32>) -> Self {
        self.max_pending_outgoing = limit;
        self
    }

    /// Configures the maximum number of concurrent established inbound connections.
    pub fn with_max_established_incoming(mut self, limit: Option<u32>) -> Self {
        self.max_established_incoming = limit;
        self
    }

    /// Configures the maximum number of concurrent established outbound connections.
    pub fn with_max_established_outgoing(mut self, limit: Option<u32>) -> Self {
        self.max_established_outgoing = limit;
        self
    }

    /// Configures the maximum number of concurrent established connections (both
    /// inbound and outbound).
    ///
    /// Note: This should be used in conjunction with
    /// [`ConnectionLimits::with_max_established_incoming`] to prevent possible
    /// eclipse attacks (all connections being inbound).
    pub fn with_max_established(mut self, limit: Option<u32>) -> Self {
        self.max_established_total = limit;
        self
    }

    /// Configures the maximum number of concurrent established connections per peer,
    /// regardless of direction (incoming or outgoing).
    pub fn with_max_established_per_peer(mut self, limit: Option<u32>) -> Self {
        self.max_established_per_peer = limit;
        self
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = dummy::ConnectionHandler;
    type ToSwarm = Void;

    fn handle_pending_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<(), ConnectionDenied> {
        check_limit(
            self.limits.max_pending_incoming,
            self.pending_inbound_connections.len(),
            Kind::PendingIncoming,
        )?;

        self.pending_inbound_connections.insert(connection_id);

        Ok(())
    }

    fn handle_established_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.pending_inbound_connections.remove(&connection_id);

        if !self.honest_peers.contains(&peer) {
            check_limit(
                self.limits.max_established_incoming,
                self.established_inbound_connections.len(),
                Kind::EstablishedIncoming,
            )?;
            check_limit(
                self.limits.max_established_total,
                self.established_inbound_connections.len()
                    + self.established_outbound_connections.len(),
                Kind::EstablishedTotal,
            )?;
        }

        check_limit(
            self.limits.max_established_per_peer,
            self.established_per_peer
                .get(&peer)
                .map(|connections| connections.len())
                .unwrap_or(0),
            Kind::EstablishedPerPeer,
        )?;

        Ok(dummy::ConnectionHandler)
    }

    fn handle_pending_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        _: Option<PeerId>,
        _: &[Multiaddr],
        _: Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        check_limit(
            self.limits.max_pending_outgoing,
            self.pending_outbound_connections.len(),
            Kind::PendingOutgoing,
        )?;

        self.pending_outbound_connections.insert(connection_id);

        Ok(vec![])
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        _: &Multiaddr,
        _: Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.pending_outbound_connections.remove(&connection_id);

        check_limit(
            self.limits.max_established_outgoing,
            self.established_outbound_connections.len(),
            Kind::EstablishedOutgoing,
        )?;
        check_limit(
            self.limits.max_established_per_peer,
            self.established_per_peer
                .get(&peer)
                .map(|connections| connections.len())
                .unwrap_or(0),
            Kind::EstablishedPerPeer,
        )?;
        check_limit(
            self.limits.max_established_total,
            self.established_inbound_connections.len()
                + self.established_outbound_connections.len(),
            Kind::EstablishedTotal,
        )?;

        Ok(dummy::ConnectionHandler)
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        match event {
            FromSwarm::ConnectionClosed(ConnectionClosed {
                peer_id,
                connection_id,
                ..
            }) => {
                self.established_inbound_connections.remove(&connection_id);
                self.established_outbound_connections.remove(&connection_id);
                self.established_per_peer
                    .entry(peer_id)
                    .or_default()
                    .remove(&connection_id);
            }
            FromSwarm::ConnectionEstablished(ConnectionEstablished {
                peer_id,
                endpoint,
                connection_id,
                ..
            }) => {
                match endpoint {
                    ConnectedPoint::Listener { .. } => {
                        self.established_inbound_connections.insert(connection_id);
                    }
                    ConnectedPoint::Dialer { .. } => {
                        self.established_outbound_connections.insert(connection_id);
                    }
                }

                self.established_per_peer
                    .entry(peer_id)
                    .or_default()
                    .insert(connection_id);
            }
            FromSwarm::DialFailure(DialFailure { connection_id, .. }) => {
                self.pending_outbound_connections.remove(&connection_id);
            }
            FromSwarm::ListenFailure(ListenFailure { connection_id, .. }) => {
                self.pending_inbound_connections.remove(&connection_id);
            }
            _ => {}
        }
    }

    fn on_connection_handler_event(
        &mut self,
        _id: PeerId,
        _: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        void::unreachable(event)
    }

    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        if let Poll::Ready(Some(Some(mut peers))) = self.pending_query.poll_unpin(cx) {
            peers.truncate(self.max_honest_peers);
            self.honest_peers = peers.into_iter().collect();
        }

        if self.pending_query.is_terminated() && self.poll_interval.poll_tick(cx).is_ready() {
            let trust_provider = Arc::clone(&self.trust_provider);
            let freshness_hours = self.freshness_days.saturating_mul(24);
            let fut: Fuse<BoxFuture<'static, Option<Vec<PeerId>>>> = async move {
                match trust_provider
                    .peers_with_financially_relevant_swap(freshness_hours)
                    .await
                {
                    Ok(peers) => Some(peers),
                    Err(e) => {
                        tracing::warn!(error = ?e, "Failed to query honest peers for connection-limit exemption");
                        None
                    }
                }
            }
            .boxed()
            .fuse();
            self.pending_query = OptionFuture::from(Some(fut));

            cx.waker().wake_by_ref();
        }

        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;

    struct FixedTrust(Vec<PeerId>);

    #[async_trait::async_trait]
    impl PeerTrust for FixedTrust {
        async fn peers_with_financially_relevant_swap(
            &self,
            _freshness_hours: u64,
        ) -> Result<Vec<PeerId>> {
            Ok(self.0.clone())
        }
    }

    struct OkThenErr {
        peers: Vec<PeerId>,
        calls: std::sync::atomic::AtomicUsize,
    }

    #[async_trait::async_trait]
    impl PeerTrust for OkThenErr {
        async fn peers_with_financially_relevant_swap(
            &self,
            _freshness_hours: u64,
        ) -> Result<Vec<PeerId>> {
            let n = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if n == 0 {
                Ok(self.peers.clone())
            } else {
                Err(anyhow::anyhow!("transient failure"))
            }
        }
    }

    fn addr() -> Multiaddr {
        "/ip4/127.0.0.1/tcp/1".parse().unwrap()
    }

    async fn refresh_honest_peers(behaviour: &mut Behaviour) {
        let waker = futures::task::noop_waker();
        let mut cx = std::task::Context::from_waker(&waker);
        for _ in 0..100 {
            let _ = behaviour.poll(&mut cx);
            if !behaviour.honest_peers.is_empty() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        panic!("honest peers were never refreshed");
    }

    #[tokio::test]
    async fn honest_peer_is_exempt_from_incoming_limit() {
        let honest = PeerId::random();
        let unknown = PeerId::random();

        let limits = ConnectionLimits::default().with_max_established_incoming(Some(0));
        let mut behaviour = Behaviour::new(
            limits,
            Arc::new(FixedTrust(vec![honest])),
            24,
            500,
            Duration::from_secs(60),
        );
        refresh_honest_peers(&mut behaviour).await;

        assert!(
            behaviour
                .handle_established_inbound_connection(
                    ConnectionId::new_unchecked(0),
                    honest,
                    &addr(),
                    &addr(),
                )
                .is_ok(),
            "honest peer must be exempt from the incoming connection limit"
        );
        assert!(
            behaviour
                .handle_established_inbound_connection(
                    ConnectionId::new_unchecked(1),
                    unknown,
                    &addr(),
                    &addr(),
                )
                .is_err(),
            "unknown peer must still be subject to the incoming connection limit"
        );
    }

    #[tokio::test]
    async fn per_peer_limit_applies_even_to_honest_peers() {
        let honest = PeerId::random();

        let limits = ConnectionLimits::default()
            .with_max_established_incoming(Some(0))
            .with_max_established_per_peer(Some(1));
        let mut behaviour = Behaviour::new(
            limits,
            Arc::new(FixedTrust(vec![honest])),
            24,
            500,
            Duration::from_secs(60),
        );
        refresh_honest_peers(&mut behaviour).await;

        assert!(
            behaviour
                .handle_established_inbound_connection(
                    ConnectionId::new_unchecked(0),
                    honest,
                    &addr(),
                    &addr(),
                )
                .is_ok()
        );
        let endpoint = ConnectedPoint::Listener {
            local_addr: addr(),
            send_back_addr: addr(),
        };
        behaviour.on_swarm_event(FromSwarm::ConnectionEstablished(ConnectionEstablished {
            peer_id: honest,
            connection_id: ConnectionId::new_unchecked(0),
            endpoint: &endpoint,
            failed_addresses: &[],
            other_established: 0,
        }));

        assert!(
            behaviour
                .handle_established_inbound_connection(
                    ConnectionId::new_unchecked(1),
                    honest,
                    &addr(),
                    &addr(),
                )
                .is_err(),
            "per-peer limit must apply even to honest peers"
        );
    }

    #[tokio::test]
    async fn honest_peer_set_is_capped() {
        let peers: Vec<PeerId> = (0..5).map(|_| PeerId::random()).collect();
        let mut behaviour = Behaviour::new(
            ConnectionLimits::default(),
            Arc::new(FixedTrust(peers)),
            24,
            2,
            Duration::from_secs(60),
        );
        refresh_honest_peers(&mut behaviour).await;

        assert_eq!(behaviour.honest_peers.len(), 2);
    }

    #[tokio::test]
    async fn honest_peers_retained_when_refresh_query_fails() {
        let honest = PeerId::random();
        let trust = Arc::new(OkThenErr {
            peers: vec![honest],
            calls: std::sync::atomic::AtomicUsize::new(0),
        });
        let mut behaviour = Behaviour::new(
            ConnectionLimits::default(),
            trust,
            24,
            500,
            Duration::from_millis(1),
        );

        refresh_honest_peers(&mut behaviour).await;
        assert!(behaviour.honest_peers.contains(&honest));

        let waker = futures::task::noop_waker();
        let mut cx = std::task::Context::from_waker(&waker);
        for _ in 0..100 {
            let _ = behaviour.poll(&mut cx);
            tokio::time::sleep(Duration::from_millis(1)).await;
        }

        assert!(
            behaviour.honest_peers.contains(&honest),
            "honest peers must survive a transient refresh failure"
        );
    }
}
