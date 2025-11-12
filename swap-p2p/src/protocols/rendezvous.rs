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
pub mod register {
    use super::*;
    use backoff::backoff::Backoff;
    use backoff::ExponentialBackoff;
    use futures::future::BoxFuture;
    use futures::stream::FuturesUnordered;
    use futures::{FutureExt, StreamExt};
    use libp2p::core::transport::PortUse;
    use libp2p::rendezvous::client::RegisterError;
    use libp2p::swarm::dial_opts::{DialOpts, PeerCondition};
    use libp2p::swarm::{
        ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, THandler, THandlerInEvent,
        THandlerOutEvent, ToSwarm,
    };
    use libp2p::{identity, Multiaddr, PeerId};
    use std::collections::HashMap;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use std::time::Duration;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ConnectionStatus {
        Disconnected,
        Dialling,
        Connected,
    }

    enum RegistrationStatus {
        RegisterOnNextConnection,
        Pending,
        Registered {
            re_register_in: Pin<Box<tokio::time::Sleep>>,
        },
    }

    pub struct Behaviour {
        inner: libp2p::rendezvous::client::Behaviour,
        rendezvous_nodes: Vec<RendezvousNode>,
        // always use schedule_dial to schedule a dial
        // do not insert directly into this future
        to_dial: FuturesUnordered<BoxFuture<'static, PeerId>>,
        backoffs: HashMap<PeerId, ExponentialBackoff>,
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
                        RegistrationStatus::Registered { .. } => {
                            RegistrationStatusReport::Registered
                        }
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
        pub fn new(identity: identity::Keypair, rendezvous_nodes: Vec<RendezvousNode>) -> Self {
            let our_peer_id = identity.public().to_peer_id();
            let rendezvous_nodes: Vec<RendezvousNode> = rendezvous_nodes
                .into_iter()
                .filter(|node| node.peer_id != our_peer_id)
                .collect();

            let mut backoffs = HashMap::new();

            // Initialize backoff for each rendezvous node
            for node in &rendezvous_nodes {
                backoffs.insert(
                    node.peer_id,
                    ExponentialBackoff {
                        // 5 minutes max interval
                        max_interval: Duration::from_secs(5 * 60),
                        // Never give up
                        max_elapsed_time: None,
                        // We retry aggressively. We begin with 50ms and increase by 10% per retry.
                        multiplier: 1.1f64,
                        initial_interval: Duration::from_millis(50),
                        current_interval: Duration::from_millis(50),
                        ..ExponentialBackoff::default()
                    },
                );
            }

            Self {
                inner: libp2p::rendezvous::client::Behaviour::new(identity),
                rendezvous_nodes,
                to_dial: FuturesUnordered::new(),
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
            self.inner.register(namespace, peer_id, ttl)
        }

        /// Schedules a dial to a peer with exponential backoff delay.
        fn schedule_dial(&mut self, peer_id: PeerId) {
            let backoff = self
                .backoffs
                .get_mut(&peer_id)
                .expect("backoff should exist for all rendezvous nodes");

            let delay = backoff
                .next_backoff()
                .expect("backoff should never run out of attempts");

            // Create a future that sleeps and then returns the peer_id
            let future = async move {
                tokio::time::sleep(delay).await;
                peer_id
            };

            self.to_dial.push(future.boxed());

            // Set the connection status to Dialling
            if let Some(node) = self
                .rendezvous_nodes
                .iter_mut()
                .find(|node| node.peer_id == peer_id)
            {
                node.set_connection(ConnectionStatus::Dialling);
            }
        }
    }

    impl NetworkBehaviour for Behaviour {
        type ConnectionHandler =
            <libp2p::rendezvous::client::Behaviour as NetworkBehaviour>::ConnectionHandler;
        type ToSwarm = libp2p::rendezvous::client::Event;

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
                        if let Some(backoff) = self.backoffs.get_mut(&peer_id) {
                            backoff.reset();
                        }

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
                        self.schedule_dial(peer_id);
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
                            self.schedule_dial(peer_id);
                        }
                    }
                }
                _ => {}
            }
            self.inner.on_swarm_event(event);
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
            // Check if we need to dial a peer
            if let Poll::Ready(Some(peer_id)) = self.to_dial.poll_next_unpin(cx) {
                // This should be redundant as this is already set in the schedule_dial function
                // we still do it here to be safe
                if let Some(node) = self
                    .rendezvous_nodes
                    .iter_mut()
                    .find(|node| node.peer_id == peer_id)
                {
                    node.set_connection(ConnectionStatus::Dialling);
                }

                return Poll::Ready(ToSwarm::Dial {
                    opts: DialOpts::peer_id(peer_id)
                        .addresses(vec![self
                            .rendezvous_nodes
                            .iter()
                            .find(|node| node.peer_id == peer_id)
                            .map(|node| node.address.clone())
                            .expect("We should have a rendezvous node for the peer id")])
                        .condition(PeerCondition::Disconnected)
                        // TODO: this makes the behaviour call `NetworkBehaviour::handle_pending_outbound_connection`
                        // but we don't implement it
                        .extend_addresses_through_behaviour()
                        .build(),
                });
            }

            // Check the status of each rendezvous node
            for i in 0..self.rendezvous_nodes.len() {
                let connection_status = self.rendezvous_nodes[i].connection_status.clone();
                match &mut self.rendezvous_nodes[i].registration_status {
                    RegistrationStatus::RegisterOnNextConnection => match connection_status {
                        ConnectionStatus::Disconnected => {
                            let peer_id = self.rendezvous_nodes[i].peer_id;
                            self.schedule_dial(peer_id);
                        }
                        ConnectionStatus::Dialling => {}
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
                                    let peer_id = self.rendezvous_nodes[i].peer_id;
                                    self.rendezvous_nodes[i].set_registration(
                                        RegistrationStatus::RegisterOnNextConnection,
                                    );
                                    self.schedule_dial(peer_id);
                                }
                                ConnectionStatus::Dialling => {}
                            }
                        }
                    }
                    RegistrationStatus::Pending => {}
                }
            }

            let inner_poll = self.inner.poll(cx);

            // reset the timer for the specific rendezvous node if we successfully registered
            if let Poll::Ready(ToSwarm::GenerateEvent(
                libp2p::rendezvous::client::Event::Registered {
                    ttl,
                    rendezvous_node,
                    ..
                },
            )) = &inner_poll
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
        // Due to an issue with the libp2p rendezvous library
        // This needs to be fixed upstream and was
        // introduced in our codebase by a libp2p refactor which bumped the version of libp2p:
        //
        // - The new bumped rendezvous client works, and can connect to an old rendezvous server
        // - The new rendezvous has an issue, which is why these test (use the new mock server)
        //   do not work
        //
        // Ignore this test for now . This works in production :)
        async fn given_no_initial_connection_when_constructed_asb_connects_and_registers_with_rendezvous_node(
        ) {
            let mut rendezvous_node = new_swarm(|_| {
                rendezvous::server::Behaviour::new(rendezvous::server::Config::default())
            });
            let address = rendezvous_node.listen_on_random_memory_address().await;
            let rendezvous_point = RendezvousNode::new(
                &address,
                rendezvous_node.local_peer_id().to_owned(),
                XmrBtcNamespace::Testnet,
                None,
            );

            let mut asb =
                new_swarm(|identity| super::Behaviour::new(identity, vec![rendezvous_point]));
            asb.listen_on_random_memory_address().await; // this adds an external address

            tokio::spawn(async move {
                loop {
                    rendezvous_node.next().await;
                }
            });
            let asb_registered = tokio::spawn(async move {
                loop {
                    if let SwarmEvent::Behaviour(rendezvous::client::Event::Registered { .. }) =
                        asb.select_next_some().await
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
        // Due to an issue with the libp2p rendezvous library
        // This needs to be fixed upstream and was
        // introduced in our codebase by a libp2p refactor which bumped the version of libp2p:
        //
        // - The new bumped rendezvous client works, and can connect to an old rendezvous server
        // - The new rendezvous has an issue, which is why these test (use the new mock server)
        //   do not work
        //
        // Ignore this test for now . This works in production :)
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

            let mut asb =
                new_swarm(|identity| super::Behaviour::new(identity, vec![rendezvous_point]));
            asb.listen_on_random_memory_address().await; // this adds an external address

            tokio::spawn(async move {
                loop {
                    rendezvous_node.next().await;
                }
            });
            let asb_registered_three_times = tokio::spawn(async move {
                let mut number_of_registrations = 0;

                loop {
                    if let SwarmEvent::Behaviour(rendezvous::client::Event::Registered { .. }) =
                        asb.select_next_some().await
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
        // Due to an issue with the libp2p rendezvous library
        // This needs to be fixed upstream and was
        // introduced in our codebase by a libp2p refactor which bumped the version of libp2p:
        //
        // - The new bumped rendezvous client works, and can connect to an old rendezvous server
        // - The new rendezvous has an issue, which is why these test (use the new mock server)
        //   do not work
        //
        // Ignore this test for now . This works in production :)
        async fn asb_registers_multiple() {
            let registration_ttl = Some(10);
            let mut rendezvous_nodes = Vec::new();
            let mut registrations = HashMap::new();
            // register with 5 rendezvous nodes
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

            let mut asb =
                new_swarm(|identity| register::Behaviour::new(identity, rendezvous_nodes));
            asb.listen_on_random_memory_address().await; // this adds an external address

            let handle = tokio::spawn(async move {
                loop {
                    if let SwarmEvent::Behaviour(rendezvous::client::Event::Registered {
                        rendezvous_node,
                        ..
                    }) = asb.select_next_some().await
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
}
