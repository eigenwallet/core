use crate::behaviour_util::BackoffTracker;
use crate::protocols::redial;

use super::*;
use futures::FutureExt;
use libp2p::rendezvous::client::RegisterError;
use libp2p::swarm::{
    ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, THandler, THandlerInEvent,
    THandlerOutEvent, ToSwarm,
};
use libp2p::{identity, rendezvous, Multiaddr, PeerId};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

// TODO: We should use the ConnectionTracker from the behaviour_util module instead of this enum
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStatus {
    Disconnected,
    Connected,
}

enum RegistrationStatus {
    RegisterOnNextConnection,
    Pending,
    Registered {
        re_register_in: Pin<Box<tokio::time::Sleep>>,
    },
}

// This could use notice to recursively discover other rendezvous nodes
pub struct Behaviour {
    inner: InnerBehaviour,
    rendezvous_nodes: Vec<RendezvousNode>,
    backoffs: BackoffTracker,
}

#[derive(NetworkBehaviour)]
pub struct InnerBehaviour {
    rendezvous: rendezvous::client::Behaviour,
    redial: redial::Behaviour,
}

// Provide a read-only snapshot of rendezvous registrations
impl Behaviour {
    /// Returns a snapshot of registration and connection status for all configured rendezvous nodes.
    pub fn registrations(&self) -> Vec<RegistrationReport> {
        self.rendezvous_nodes
            .iter()
            .map(|n| RegistrationReport {
                address: n.address.clone(),
                connection: n.connection_status,
                registration: match &n.registration_status {
                    RegistrationStatus::RegisterOnNextConnection => {
                        RegistrationStatusReport::RegisterOnNextConnection
                    }
                    RegistrationStatus::Pending => RegistrationStatusReport::Pending,
                    RegistrationStatus::Registered { .. } => RegistrationStatusReport::Registered,
                },
            })
            .collect()
    }
}

/// Public representation of a rendezvous node registration status
/// The raw `RegistrationStatus` cannot be exposed because it is not serializable
#[derive(Debug, Clone)]
pub struct RegistrationReport {
    pub address: Multiaddr,
    pub connection: ConnectionStatus,
    pub registration: RegistrationStatusReport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrationStatusReport {
    RegisterOnNextConnection,
    Pending,
    Registered,
}

/// A node running the rendezvous server protocol.
pub struct RendezvousNode {
    pub address: Multiaddr,
    connection_status: ConnectionStatus,
    pub peer_id: PeerId,
    registration_status: RegistrationStatus,
    pub registration_ttl: Option<u64>,
    pub namespace: XmrBtcNamespace,
}

impl RendezvousNode {
    pub fn new(
        address: &Multiaddr,
        peer_id: PeerId,
        namespace: XmrBtcNamespace,
        registration_ttl: Option<u64>,
    ) -> Self {
        Self {
            address: address.to_owned(),
            connection_status: ConnectionStatus::Disconnected,
            namespace,
            peer_id,
            registration_status: RegistrationStatus::RegisterOnNextConnection,
            registration_ttl,
        }
    }

    fn set_connection(&mut self, status: ConnectionStatus) {
        self.connection_status = status;
    }

    fn set_registration(&mut self, status: RegistrationStatus) {
        self.registration_status = status;
    }
}

impl Behaviour {
    const REDIAL_IDENTIFIER: &str = "rendezvous-register";

    pub fn new(identity: identity::Keypair, rendezvous_nodes: Vec<RendezvousNode>) -> Self {
        let our_peer_id = identity.public().to_peer_id();
        let rendezvous_nodes: Vec<RendezvousNode> = rendezvous_nodes
            .into_iter()
            .filter(|node| node.peer_id != our_peer_id)
            .collect();

        let backoffs = BackoffTracker::new(
            crate::defaults::RENDEZVOUS_RETRY_INITIAL_INTERVAL,
            crate::defaults::RENDEZVOUS_RETRY_MAX_INTERVAL,
            1.1f64,
        );

        let mut redial = redial::Behaviour::new(
            Self::REDIAL_IDENTIFIER,
            crate::defaults::REDIAL_INITIAL_INTERVAL,
            crate::defaults::REDIAL_MAX_INTERVAL,
        );

        // Initialize backoff for each rendezvous node
        for node in &rendezvous_nodes {
            redial.add_peer_with_address(node.peer_id, node.address.clone());
        }

        Self {
            inner: InnerBehaviour {
                rendezvous: rendezvous::client::Behaviour::new(identity),
                redial,
            },
            rendezvous_nodes,
            backoffs,
        }
    }

    /// Registers the rendezvous node at the given index.
    /// Also sets the registration status to [`RegistrationStatus::Pending`].
    pub fn register(&mut self, node_index: usize) -> Result<(), RegisterError> {
        let node = &mut self.rendezvous_nodes[node_index];
        node.set_registration(RegistrationStatus::Pending);
        let (namespace, peer_id, ttl) =
            (node.namespace.into(), node.peer_id, node.registration_ttl);
        self.inner.rendezvous.register(namespace, peer_id, ttl)
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = <InnerBehaviour as NetworkBehaviour>::ConnectionHandler;
    type ToSwarm = InnerBehaviourEvent;

    fn on_swarm_event(&mut self, event: FromSwarm<'_>) {
        match event {
            FromSwarm::ConnectionEstablished(connection) => {
                let peer_id = connection.peer_id;

                // Find the rendezvous node that matches the peer id, else do nothing.
                if let Some(index) = self
                    .rendezvous_nodes
                    .iter_mut()
                    .position(|node| node.peer_id == peer_id)
                {
                    let rendezvous_node = &mut self.rendezvous_nodes[index];
                    rendezvous_node.set_connection(ConnectionStatus::Connected);

                    // Reset backoff on successful connection
                    self.backoffs.reset(&peer_id);

                    if let RegistrationStatus::RegisterOnNextConnection =
                        rendezvous_node.registration_status
                    {
                        let _ = self.register(index).inspect_err(|err| {
                            tracing::error!(
                                    error=%err,
                                    rendezvous_node=%peer_id,
                                    "Failed to register with rendezvous node");
                        });
                    }
                }
            }
            FromSwarm::ConnectionClosed(connection) => {
                let peer_id = connection.peer_id;

                // Update the connection status of the rendezvous node that disconnected.
                if let Some(node) = self
                    .rendezvous_nodes
                    .iter_mut()
                    .find(|node| node.peer_id == peer_id)
                {
                    node.set_connection(ConnectionStatus::Disconnected);
                }
            }
            FromSwarm::DialFailure(dial_failure) => {
                // Update the connection status of the rendezvous node that failed to connect.
                if let Some(peer_id) = dial_failure.peer_id {
                    if let Some(node) = self
                        .rendezvous_nodes
                        .iter_mut()
                        .find(|node| node.peer_id == peer_id)
                    {
                        node.set_connection(ConnectionStatus::Disconnected);
                    }
                }
            }
            _ => {}
        }
        self.inner.on_swarm_event(event);
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        // Check the status of each rendezvous node
        for i in 0..self.rendezvous_nodes.len() {
            let connection_status = self.rendezvous_nodes[i].connection_status.clone();
            match &mut self.rendezvous_nodes[i].registration_status {
                RegistrationStatus::RegisterOnNextConnection => match connection_status {
                    ConnectionStatus::Disconnected => {}
                    ConnectionStatus::Connected => {
                        let _ = self.register(i);
                    }
                },
                RegistrationStatus::Registered { re_register_in } => {
                    if let Poll::Ready(()) = re_register_in.poll_unpin(cx) {
                        match connection_status {
                            ConnectionStatus::Connected => {
                                let _ = self.register(i).inspect_err(|err| {
                                    tracing::error!(
                                            error=%err,
                                            rendezvous_node=%self.rendezvous_nodes[i].peer_id,
                                            "Failed to register with rendezvous node");
                                });
                            }
                            ConnectionStatus::Disconnected => {
                                self.rendezvous_nodes[i]
                                    .set_registration(RegistrationStatus::RegisterOnNextConnection);
                            }
                        }
                    }
                }
                RegistrationStatus::Pending => {}
            }
        }

        let inner_poll = self.inner.poll(cx);

        // Reset the timer for the specific rendezvous node if we successfully registered
        if let Poll::Ready(ToSwarm::GenerateEvent(InnerBehaviourEvent::Rendezvous(
            rendezvous::client::Event::Registered {
                ttl,
                rendezvous_node,
                ..
            },
        ))) = &inner_poll
        {
            if let Some(i) = self
                .rendezvous_nodes
                .iter()
                .position(|n| &n.peer_id == rendezvous_node)
            {
                let half_of_ttl = Duration::from_secs(*ttl) / 2;
                let re_register_in = Box::pin(tokio::time::sleep(half_of_ttl));
                let status = RegistrationStatus::Registered { re_register_in };
                self.rendezvous_nodes[i].set_registration(status);
            }
        }

        inner_poll
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

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        addr: &Multiaddr,
        role_override: libp2p::core::Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.inner
            .handle_established_outbound_connection(connection_id, peer, addr, role_override)
    }

    fn handle_pending_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        maybe_peer: Option<PeerId>,
        addresses: &[Multiaddr],
        effective_role: libp2p::core::Endpoint,
    ) -> std::result::Result<Vec<Multiaddr>, ConnectionDenied> {
        self.inner.handle_pending_outbound_connection(
            connection_id,
            maybe_peer,
            addresses,
            effective_role,
        )
    }

    fn handle_pending_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<(), ConnectionDenied> {
        self.inner
            .handle_pending_inbound_connection(connection_id, local_addr, remote_addr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::{new_swarm, SwarmExt};
    use futures::StreamExt;
    use libp2p::rendezvous;
    use libp2p::swarm::SwarmEvent;
    use std::collections::HashMap;

    #[tokio::test]
    async fn given_no_initial_connection_when_constructed_asb_connects_and_registers_with_rendezvous_node(
    ) {
        let mut rendezvous_node =
            new_swarm(
                |_| rendezvous::server::Behaviour::new(rendezvous::server::Config::default()),
            );
        let address = rendezvous_node.listen_on_random_memory_address().await;
        let rendezvous_point = RendezvousNode::new(
            &address,
            rendezvous_node.local_peer_id().to_owned(),
            XmrBtcNamespace::Testnet,
            None,
        );

        let mut asb = new_swarm(|identity| super::Behaviour::new(identity, vec![rendezvous_point]));
        asb.listen_on_random_memory_address().await;

        tokio::spawn(async move {
            loop {
                rendezvous_node.next().await;
            }
        });
        let asb_registered = tokio::spawn(async move {
            loop {
                if let SwarmEvent::Behaviour(InnerBehaviourEvent::Rendezvous(
                    rendezvous::client::Event::Registered { .. },
                )) = asb.select_next_some().await
                {
                    break;
                }
            }
        });

        tokio::time::timeout(Duration::from_secs(10), asb_registered)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn asb_automatically_re_registers() {
        let mut rendezvous_node = new_swarm(|_| {
            rendezvous::server::Behaviour::new(
                rendezvous::server::Config::default().with_min_ttl(2),
            )
        });
        let address = rendezvous_node.listen_on_random_memory_address().await;
        let rendezvous_point = RendezvousNode::new(
            &address,
            rendezvous_node.local_peer_id().to_owned(),
            XmrBtcNamespace::Testnet,
            Some(5),
        );

        let mut asb = new_swarm(|identity| super::Behaviour::new(identity, vec![rendezvous_point]));
        asb.listen_on_random_memory_address().await;

        tokio::spawn(async move {
            loop {
                rendezvous_node.next().await;
            }
        });
        let asb_registered_three_times = tokio::spawn(async move {
            let mut number_of_registrations = 0;

            loop {
                if let SwarmEvent::Behaviour(InnerBehaviourEvent::Rendezvous(
                    rendezvous::client::Event::Registered { .. },
                )) = asb.select_next_some().await
                {
                    number_of_registrations += 1
                }

                if number_of_registrations == 3 {
                    break;
                }
            }
        });

        tokio::time::timeout(Duration::from_secs(30), asb_registered_three_times)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn asb_registers_multiple() {
        let registration_ttl = Some(10);
        let mut rendezvous_nodes = Vec::new();
        let mut registrations = HashMap::new();

        // Register with 5 rendezvous nodes
        for _ in 0..5 {
            let mut rendezvous = new_swarm(|_| {
                rendezvous::server::Behaviour::new(
                    rendezvous::server::Config::default().with_min_ttl(2),
                )
            });
            let address = rendezvous.listen_on_random_memory_address().await;
            let id = *rendezvous.local_peer_id();
            registrations.insert(id, 0);
            rendezvous_nodes.push(RendezvousNode::new(
                &address,
                *rendezvous.local_peer_id(),
                XmrBtcNamespace::Testnet,
                registration_ttl,
            ));
            tokio::spawn(async move {
                loop {
                    rendezvous.next().await;
                }
            });
        }

        let mut asb = new_swarm(|identity| register::Behaviour::new(identity, rendezvous_nodes));
        asb.listen_on_random_memory_address().await; // this adds an external address

        let handle = tokio::spawn(async move {
            loop {
                if let SwarmEvent::Behaviour(InnerBehaviourEvent::Rendezvous(
                    rendezvous::client::Event::Registered {
                        rendezvous_node, ..
                    },
                )) = asb.select_next_some().await
                {
                    registrations
                        .entry(rendezvous_node)
                        .and_modify(|counter| *counter += 1);
                }

                if registrations.iter().all(|(_, &count)| count >= 4) {
                    break;
                }
            }
        });

        tokio::time::timeout(Duration::from_secs(30), handle)
            .await
            .unwrap()
            .unwrap();
    }
}
