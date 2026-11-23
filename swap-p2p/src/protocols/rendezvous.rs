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
    use crate::test::{SwarmExt, new_swarm};
    use futures::StreamExt;
    use libp2p::rendezvous;
    use libp2p::swarm::SwarmEvent;
    use libp2p::{Multiaddr, PeerId};
    use std::time::Duration;

    #[tokio::test]
    async fn register_and_discover_together() {
        // Create rendezvous node
        let (rendezvous_peer_id, rendezvous_addr, rendezvous_handle) =
            spawn_rendezvous_node().await;

        // Create peer that registers at the rendezvous node
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

        // Create peer that discovers the
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
                    _ => {}
                }

                if saw_discovery && saw_address {
                    break;
                }
            }
        });

        tokio::time::timeout(Duration::from_secs(10), discovery_task)
            .await
            .expect("discovery and direct connection to registrar timed out")
            .unwrap();

        registrar_task.abort();
        rendezvous_handle.abort();
    }

    /// A taker whose first discovery request returns no registrations must
    /// still ask again after `DISCOVERY_INTERVAL` and find makers that
    /// registered in the meantime.
    ///
    /// Takes about `DISCOVERY_INTERVAL` (60s) of real time.
    #[tokio::test]
    async fn discover_again_after_empty_discovery() {
        let (rendezvous_peer_id, rendezvous_addr, mut rendezvous_events, rendezvous_handle) =
            spawn_rendezvous_node_with_events().await;

        let mut discoverer = new_swarm(|identity| {
            discovery::Behaviour::new(
                identity,
                vec![rendezvous_peer_id],
                XmrBtcNamespace::Testnet.into(),
            )
        });
        discoverer.add_peer_address(rendezvous_peer_id, rendezvous_addr.clone());

        let (discovered_sender, mut discovered) = tokio::sync::mpsc::unbounded_channel();
        let discoverer_task = tokio::spawn(async move {
            loop {
                if let SwarmEvent::Behaviour(discovery::Event::DiscoveredPeer { peer_id }) =
                    discoverer.select_next_some().await
                {
                    let _ = discovered_sender.send(peer_id);
                }
            }
        });

        // Wait until the rendezvous node answered the first discovery request
        // with an empty list
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(rendezvous::server::Event::DiscoverServed { registrations, .. }) =
                    rendezvous_events.recv().await
                {
                    assert!(registrations.is_empty());
                    return;
                }
            }
        })
        .await
        .expect("rendezvous node did not serve the first discovery request");

        // Only now does a maker register at the rendezvous node
        let mut registrar = new_swarm(|identity| {
            register::Behaviour::new(
                identity,
                vec![rendezvous_peer_id],
                XmrBtcNamespace::Testnet.into(),
            )
        });
        registrar.add_peer_address(rendezvous_peer_id, rendezvous_addr);
        registrar.listen_on_random_memory_address().await;
        let registrar_id = *registrar.local_peer_id();

        let registrar_task = tokio::spawn(async move {
            loop {
                registrar.next().await;
            }
        });

        // The taker must discover the maker with its next scheduled request
        tokio::time::timeout(
            crate::defaults::DISCOVERY_INTERVAL + Duration::from_secs(10),
            async {
                while let Some(peer_id) = discovered.recv().await {
                    if peer_id == registrar_id {
                        return;
                    }
                }
            },
        )
        .await
        .expect("taker did not discover the maker after an empty discovery");

        discoverer_task.abort();
        registrar_task.abort();
        rendezvous_handle.abort();
    }

    /// Spawns a rendezvous server that continuously processes events
    async fn spawn_rendezvous_node() -> (PeerId, Multiaddr, tokio::task::JoinHandle<()>) {
        let (peer_id, address, _events, handle) = spawn_rendezvous_node_with_events().await;

        (peer_id, address, handle)
    }

    /// Like [`spawn_rendezvous_node`], but also forwards the server's events
    async fn spawn_rendezvous_node_with_events() -> (
        PeerId,
        Multiaddr,
        tokio::sync::mpsc::UnboundedReceiver<rendezvous::server::Event>,
        tokio::task::JoinHandle<()>,
    ) {
        let mut rendezvous_node = new_swarm(|_| {
            rendezvous::server::Behaviour::new(
                rendezvous::server::Config::default().with_min_ttl(2),
            )
        });
        let address = rendezvous_node.listen_on_random_memory_address().await;
        let peer_id = *rendezvous_node.local_peer_id();
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();

        let handle = tokio::spawn(async move {
            loop {
                if let SwarmEvent::Behaviour(event) = rendezvous_node.select_next_some().await {
                    let _ = sender.send(event);
                }
            }
        });

        (peer_id, address, receiver, handle)
    }
}
