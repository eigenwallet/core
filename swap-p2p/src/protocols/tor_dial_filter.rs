//! Removes onion addresses from the dial candidates when Tor is disabled.
//!
//! libp2p builds the address list of a dial from the
//! [`NetworkBehaviour::handle_pending_outbound_connection`] results of all
//! behaviours. [`TorDialFilter`] wraps the top-level behaviour and filters that
//! combined list. Addresses that the caller puts into `DialOpts` are not
//! filtered.

use std::ops::{Deref, DerefMut};
use std::task::{Context, Poll};

use libp2p::PeerId;
use libp2p::core::transport::PortUse;
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

/// Wraps a [`NetworkBehaviour`] and removes onion addresses from its dial
/// candidates when Tor is disabled.
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

        if self.tor_enabled {
            return Ok(candidates);
        }

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
        port_use: PortUse,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.inner.handle_established_outbound_connection(
            connection_id,
            peer,
            addr,
            role_override,
            port_use,
        )
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
