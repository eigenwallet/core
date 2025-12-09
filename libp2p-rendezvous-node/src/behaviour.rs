use anyhow::Result;
use libp2p::swarm::NetworkBehaviour;
use libp2p::{identity, PeerId};
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
        rendezvous_nodes: Vec<PeerId>,
        namespace: XmrBtcNamespace,
    ) -> Result<Self> {
        let server = libp2p::rendezvous::server::Behaviour::new(
            libp2p::rendezvous::server::Config::default(),
        );

        let register = register::Behaviour::new(identity, rendezvous_nodes, namespace.into());

        Ok(Self { server, register })
    }
}
