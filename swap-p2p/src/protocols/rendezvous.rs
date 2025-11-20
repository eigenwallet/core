use libp2p::rendezvous::Namespace;
use std::fmt;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum XmrBtcNamespace {
    Mainnet,
    Testnet,
    RendezvousPoint,
}

const MAINNET: &str = "xmr-btc-swap-mainnet";
const TESTNET: &str = "xmr-btc-swap-testnet";
const RENDEZVOUS_POINT: &str = "rendezvous-point";

impl fmt::Display for XmrBtcNamespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            XmrBtcNamespace::Mainnet => write!(f, "{}", MAINNET),
            XmrBtcNamespace::Testnet => write!(f, "{}", TESTNET),
            XmrBtcNamespace::RendezvousPoint => write!(f, "{}", RENDEZVOUS_POINT),
        }
    }
}

impl From<XmrBtcNamespace> for Namespace {
    fn from(namespace: XmrBtcNamespace) -> Self {
        match namespace {
            XmrBtcNamespace::Mainnet => Namespace::from_static(MAINNET),
            XmrBtcNamespace::Testnet => Namespace::from_static(TESTNET),
            XmrBtcNamespace::RendezvousPoint => Namespace::from_static(RENDEZVOUS_POINT),
        }
    }
}

impl XmrBtcNamespace {
    pub fn from_is_testnet(testnet: bool) -> XmrBtcNamespace {
        if testnet {
            XmrBtcNamespace::Testnet
        } else {
            XmrBtcNamespace::Mainnet
        }
    }
}

/// A behaviour that periodically re-registers at multiple rendezvous points as a client
pub mod register;

/// A behaviour that periodically discovers other peers at a given rendezvous point
///
/// The behaviour also internally attempts to dial any newly discovered peers
/// It uses the `redial` behaviour internally to do this
pub mod discovery;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::{new_swarm, SwarmExt};
    use futures::StreamExt;
    use libp2p::rendezvous;
    use libp2p::swarm::SwarmEvent;
    use libp2p::{Multiaddr, PeerId};
    use std::time::Duration;

    #[tokio::test]
    async fn register_and_discover_together() {
        let (rendezvous_peer_id, rendezvous_addr, rendezvous_handle) =
            spawn_rendezvous_server().await;

        // Registering client (adds an external address so it can be discovered)
        let mut registrar = new_swarm(|identity| {
            register::Behaviour::new(
                identity,
                vec![rendezvous_peer_id],
                XmrBtcNamespace::Testnet.into(),
            )
        });
        registrar.add_peer_address(rendezvous_peer_id, rendezvous_addr.clone());
        registrar.listen_on_random_memory_address().await;
        let registrar_id = *registrar.local_peer_id();

        // Discovery client using our wrapper behaviour
        let mut discoverer = new_swarm(|identity| {
            discovery::Behaviour::new(
                identity,
                vec![rendezvous_peer_id],
                XmrBtcNamespace::Testnet.into(),
            )
        });
        discoverer.add_peer_address(rendezvous_peer_id, rendezvous_addr);

        let registrar_task = tokio::spawn(async move {
            loop {
                registrar.next().await;
            }
        });

        // Now wait until discovery wrapper discovers registrar and dials it.
        let discovery_task = tokio::spawn(async move {
            let mut saw_discovery = false;
            let mut saw_address = false;

            loop {
                match discoverer.select_next_some().await {
                    SwarmEvent::Behaviour(discovery::Event::DiscoveredPeer { peer_id })
                        if peer_id == registrar_id =>
                    {
                        saw_discovery = true;
                    }
                    SwarmEvent::NewExternalAddrOfPeer { peer_id, .. }
                        if peer_id == registrar_id =>
                    {
                        saw_address = true;
                    }
                    SwarmEvent::ConnectionEstablished { peer_id, .. }
                        if peer_id == registrar_id && saw_discovery && saw_address =>
                    {
                        break;
                    }
                    _ => {}
                }
            }
        });

        tokio::time::timeout(Duration::from_secs(60), discovery_task)
            .await
            .expect("discovery and direct connection to registrar timed out")
            .unwrap();

        registrar_task.abort();
        rendezvous_handle.abort();
    }

    /// Spawns a rendezvous server that continuously processes events
    async fn spawn_rendezvous_server() -> (PeerId, Multiaddr, tokio::task::JoinHandle<()>) {
        let mut rendezvous_node = new_swarm(|_| {
            rendezvous::server::Behaviour::new(
                rendezvous::server::Config::default().with_min_ttl(2),
            )
        });
        let address = rendezvous_node.listen_on_random_memory_address().await;
        let peer_id = *rendezvous_node.local_peer_id();

        let handle = tokio::spawn(async move {
            loop {
                rendezvous_node.next().await;
            }
        });

        (peer_id, address, handle)
    }
}
