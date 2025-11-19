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

// TODO: Rework these tests, they are ugly
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::{new_swarm, SwarmExt};
    use futures::StreamExt;
    use libp2p::rendezvous;
    use libp2p::swarm::SwarmEvent;
    use std::time::Duration;

    // Helper to spawn a background poller for a swarm that just drains events.
    fn spawn_drain_swarm<B>(mut swarm: libp2p::Swarm<B>)
    where
        B: libp2p::swarm::NetworkBehaviour + Send + 'static,
        <B as libp2p::swarm::NetworkBehaviour>::ToSwarm: std::fmt::Debug,
    {
        tokio::spawn(async move {
            loop {
                let _ = swarm.next().await;
            }
        });
    }

    #[tokio::test]
    async fn register_and_discover_together() {
        // Rendezvous server
        let mut rendezvous_server =
            new_swarm(
                |_| rendezvous::server::Behaviour::new(rendezvous::server::Config::default()),
            );
        let server_addr = rendezvous_server.listen_on_random_memory_address().await;
        let server_id = *rendezvous_server.local_peer_id();

        // Registering client (adds an external address so it can be discovered)
        let mut registrar = new_swarm(|identity| {
            register::Behaviour::new(
                identity,
                vec![register::RendezvousNode::new(
                    &server_addr,
                    server_id,
                    XmrBtcNamespace::Testnet,
                    Some(10),
                )],
            )
        });
        registrar.listen_on_random_memory_address().await;
        let registrar_id = *registrar.local_peer_id();

        // Discovery client using our wrapper behaviour
        let mut discoverer = new_swarm(|identity| {
            discovery::Behaviour::new(identity, vec![server_id], XmrBtcNamespace::Testnet.into())
        });

        // First connect registrar to server to ensure it can register promptly.
        registrar.block_on_connection(&mut rendezvous_server).await;
        // Then connect discoverer to the rendezvous server without poking inner behaviours.
        discoverer.block_on_connection(&mut rendezvous_server).await;

        // Drive server in background and observe registrar until it registers once.
        spawn_drain_swarm(rendezvous_server);
        let (tx_reg, rx_reg) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(async move {
            let mut registrar = registrar;
            let mut sent = false;
            let mut tx_opt = Some(tx_reg);
            loop {
                match registrar.select_next_some().await {
                    SwarmEvent::Behaviour(register::InnerBehaviourEvent::Rendezvous(
                        rendezvous::client::Event::Registered { .. },
                    )) if !sent => {
                        if let Some(sender) = tx_opt.take() {
                            let _ = sender.send(());
                        }
                        sent = true;
                    }
                    _ => {}
                }
            }
        });
        tokio::time::timeout(Duration::from_secs(30), rx_reg)
            .await
            .expect("registrar did not register in time")
            .ok();

        // Now wait until discovery wrapper discovers registrar and dials it.
        let _ = tokio::time::timeout(Duration::from_secs(60), async {
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
        })
        .await
        .expect("discovery and direct connection to registrar timed out");
    }
}
