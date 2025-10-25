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
        registration_ttl: Option<u64>,
    ) -> Result<Self> {
        let server = libp2p::rendezvous::server::Behaviour::new(
            libp2p::rendezvous::server::Config::default(),
        );

        let rendezvous_nodes =
            build_rendezvous_nodes(rendezvous_addrs, namespace, registration_ttl)?;
        let register = register::Behaviour::new(identity, rendezvous_nodes);

        Ok(Self { server, register })
    }
}

/// Builds a list of RendezvousNode from multiaddrs and namespace
fn build_rendezvous_nodes(
    addrs: Vec<Multiaddr>,
    namespace: XmrBtcNamespace,
    registration_ttl: Option<u64>,
) -> Result<Vec<register::RendezvousNode>> {
    addrs
        .into_iter()
        .map(|addr| {
            let peer_id = extract_peer_id(&addr)?;
            Ok(register::RendezvousNode::new(
                &addr,
                peer_id,
                namespace,
                registration_ttl,
            ))
        })
        .collect()
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
