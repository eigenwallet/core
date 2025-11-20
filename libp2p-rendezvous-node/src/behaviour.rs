use anyhow::{Context, Result};
use libp2p::swarm::NetworkBehaviour;
use libp2p::{identity, Multiaddr, PeerId};
use swap_p2p::protocols::rendezvous::{register, XmrBtcNamespace};

/// Acts as both a rendezvous server and registers at other rendezvous points
#[derive(NetworkBehaviour)]
pub struct Behaviour {
    pub server: libp2p::rendezvous::server::Behaviour,
    pub register: register::Behaviour,
}

impl Behaviour {
    pub fn new(
        identity: identity::Keypair,
        rendezvous_addrs: Vec<Multiaddr>,
        namespace: XmrBtcNamespace,
    ) -> Result<Self> {
        let server = libp2p::rendezvous::server::Behaviour::new(
            libp2p::rendezvous::server::Config::default(),
        );

        let rendezvous_nodes = rendezvous_addrs
            .iter()
            .map(|addr| extract_peer_id(addr))
            .collect::<Result<Vec<_>>>()?;

        let register = register::Behaviour::new(identity, rendezvous_nodes, namespace.into());

        Ok(Self { server, register })
    }
}

fn extract_peer_id(addr: &Multiaddr) -> Result<PeerId> {
    addr.iter()
        .find_map(|protocol| {
            if let libp2p::multiaddr::Protocol::P2p(peer_id) = protocol {
                Some(peer_id)
            } else {
                None
            }
        })
        .context("No peer_id found in multiaddr")
}
