//! A single choke point that prevents dialing onion addresses without Tor.
//!
//! [`TorDialFilter`] wraps any [`NetworkBehaviour`] and, when Tor is disabled,
//! strips `/onion3` (and `/onion`) addresses from every dial-candidate list the
//! inner behaviour produces via [`NetworkBehaviour::handle_pending_outbound_connection`].
//!
//! Why a wrapper instead of per-behaviour filters?
//!
//! libp2p assembles the address list for an address-less `DialOpts` by calling
//! `handle_pending_outbound_connection` on *every* behaviour and concatenating
//! the results. Many behaviours contribute cached peer addresses here: the
//! `redial` behaviours, `identify`, and — crucially — every libp2p
//! `request_response` behaviour keeps a per-peer address book that it populates
//! from `FromSwarm::NewExternalAddrOfPeer` and hands back as dial candidates.
//! Filtering each of them individually is fragile whack-a-mole. Wrapping the
//! top-level behaviour filters the *combined* result once, so no matter which
//! inner behaviour contributed an onion address it is dropped before the Swarm
//! ever hands it to the transport.
//!
//! It delegates everything else to the inner behaviour (including, via
//! [`Deref`], its inherent API and derived field access).

use std::ops::{Deref, DerefMut};
use std::task::{Context, Poll};

use libp2p::PeerId;
use libp2p::core::{Endpoint, Multiaddr};
use libp2p::multiaddr::Protocol;
use libp2p::swarm::{
    ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, THandler, THandlerInEvent,
    THandlerOutEvent, ToSwarm,
};

/// Returns true if the multiaddr contains a Tor onion (v2 or v3) component.
fn is_onion(addr: &Multiaddr) -> bool {
    addr.iter()
        .any(|proto| matches!(proto, Protocol::Onion(_, _) | Protocol::Onion3(_)))
}

/// Wraps a [`NetworkBehaviour`] and drops onion dial candidates when Tor is off.
///
/// When `tor_enabled` is `true` this is a transparent passthrough.
#[allow(missing_debug_implementations)]
pub struct TorDialFilter<B> {
    inner: B,
    tor_enabled: bool,
}

impl<B> TorDialFilter<B> {
    pub fn new(inner: B, tor_enabled: bool) -> Self {
        Self { inner, tor_enabled }
    }
}

impl<B> Deref for TorDialFilter<B> {
    type Target = B;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<B> DerefMut for TorDialFilter<B> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<B> NetworkBehaviour for TorDialFilter<B>
where
    B: NetworkBehaviour,
{
    type ConnectionHandler = B::ConnectionHandler;
    type ToSwarm = B::ToSwarm;

    fn handle_pending_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<(), ConnectionDenied> {
        self.inner
            .handle_pending_inbound_connection(connection_id, local_addr, remote_addr)
    }

    fn handle_established_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.inner.handle_established_inbound_connection(
            connection_id,
            peer,
            local_addr,
            remote_addr,
        )
    }

    fn handle_pending_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        maybe_peer: Option<PeerId>,
        addresses: &[Multiaddr],
        effective_role: Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        let candidates = self.inner.handle_pending_outbound_connection(
            connection_id,
            maybe_peer,
            addresses,
            effective_role,
        )?;

        // With Tor enabled every address is dialable, so don't touch anything.
        if self.tor_enabled {
            return Ok(candidates);
        }

        // Tor is off: drop onion addresses so the Swarm never even attempts to
        // dial them (an onion dial without Tor can only fail with
        // `MultiaddrNotSupported` and trigger pointless redial churn).
        Ok(candidates
            .into_iter()
            .filter(|addr| !is_onion(addr))
            .collect())
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        addr: &Multiaddr,
        role_override: Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.inner
            .handle_established_outbound_connection(connection_id, peer, addr, role_override)
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        self.inner.on_swarm_event(event)
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        self.inner
            .on_connection_handler_event(peer_id, connection_id, event)
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        self.inner.poll(cx)
    }
}
