use std::task::Poll;

use libp2p::identify;
use libp2p::PeerId;

use crate::libp2p_ext::MultiAddrExt;

/// This wraps libp2p::identify::Behaviour, and:
/// 1. Blocks Identify from sharing local addresses with other peers
/// 2. Blocks Identify from sharing addresses of other peers with the Swarm
/// 
/// This helps with:
/// 1. privacy (by avoiding to share local addresses with other peers)
/// 2. preventing the Swarm from trying to dial addresses that we probably cannot reach anyway
/// 
/// TODO: Add a clipply rule to forbid the normal identify behaviour from being used in the codebase
pub struct Behaviour {
    inner: identify::Behaviour,
}

impl Behaviour {
    pub fn new(config: identify::Config) -> Self {
        Self {
            inner: identify::Behaviour::new(config),
        }
    }
}

impl libp2p::swarm::NetworkBehaviour for Behaviour {
    type ConnectionHandler = <identify::Behaviour as libp2p::swarm::NetworkBehaviour>::ConnectionHandler;
    type ToSwarm = <identify::Behaviour as libp2p::swarm::NetworkBehaviour>::ToSwarm;

    fn handle_established_inbound_connection(
        &mut self,
        connection_id: libp2p::swarm::ConnectionId,
        peer: PeerId,
        local_addr: &libp2p::Multiaddr,
        remote_addr: &libp2p::Multiaddr,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        self.inner.handle_established_inbound_connection(connection_id, peer, local_addr, remote_addr)
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: libp2p::swarm::ConnectionId,
        peer: PeerId,
        addr: &libp2p::Multiaddr,
        role_override: libp2p::core::Endpoint,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        self.inner.handle_established_outbound_connection(connection_id, peer, addr, role_override)
    }

    fn on_swarm_event(&mut self, event: libp2p::swarm::FromSwarm) {
        match event {
            libp2p::swarm::FromSwarm::NewListenAddr(new_listen_addr) if new_listen_addr.addr.is_local() => {
                tracing::trace!(?new_listen_addr, "Blocking attempt by Swarm to tell Identify to share local address with other peers (FromSwarm::NewListenAddr)");
            }
            libp2p::swarm::FromSwarm::NewExternalAddrCandidate(new_external_addr_candidate) if new_external_addr_candidate.addr.is_local() => {
                tracing::trace!(?new_external_addr_candidate, "Blocking attempt by Swarm to tell Identify to share a local address with the Swarm (FromSwarm::NewExternalAddrCandidate)");
            }
            libp2p::swarm::FromSwarm::NewExternalAddrCandidate(new_external_addr_candidate) => {
                tracing::trace!(?new_external_addr_candidate, "Blocking attempt by Swarm to tell Identify to share a local address with the Swarm (FromSwarm::NewExternalAddrCandidate)");
            }
            other => self.inner.on_swarm_event(other),
        }
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        connection_id: libp2p::swarm::ConnectionId,
        event: libp2p::swarm::THandlerOutEvent<Self>,
    ) {
        self.inner.on_connection_handler_event(peer_id, connection_id, event);
    }
    
    fn poll(&mut self, cx: &mut std::task::Context<'_>)
        -> std::task::Poll<libp2p::swarm::ToSwarm<Self::ToSwarm, libp2p::swarm::THandlerInEvent<Self>>> {
        while let Poll::Ready(event) = self.inner.poll(cx) {
            match event {
                // We ignore the private addresses that other peers tell us through Identify
                libp2p::swarm::ToSwarm::NewExternalAddrOfPeer { peer_id, address } if address.is_local() => {
                    tracing::trace!(?peer_id, ?address, "Blocking attempt by Identify to share a local address of another peer with the Swarm");
                    continue;
                }
                _ => return Poll::Ready(event),
            }
        }

        Poll::Pending
    }
}