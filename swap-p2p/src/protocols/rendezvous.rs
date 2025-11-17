use libp2p::rendezvous::Namespace;
use std::{fmt, time::Duration};

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

const REDIAL_INITIAL_INTERVAL: Duration = Duration::from_secs(1);
const REDIAL_MAX_INTERVAL: Duration = Duration::from_secs(60);

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
    use crate::behaviour_util::BackoffTracker;
    use crate::protocols::redial;

    use super::*;
    use backoff::backoff::Backoff;
    use backoff::ExponentialBackoff;
    use futures::FutureExt;
    use libp2p::rendezvous::client::RegisterError;
    use libp2p::swarm::{
        ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, THandler, THandlerInEvent,
        THandlerOutEvent, ToSwarm,
    };
    use libp2p::{identity, rendezvous, Multiaddr, PeerId};
    use std::collections::HashMap;
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
        const RENDEZVOUS_RETRY_INITIAL_INTERVAL: Duration = Duration::from_secs(1);
        const RENDEZVOUS_RETRY_MAX_INTERVAL: Duration = Duration::from_secs(60);
        const REDIAL_IDENTIFIER: &str = "rendezvous-server-for-register";

        pub fn new(identity: identity::Keypair, rendezvous_nodes: Vec<RendezvousNode>) -> Self {
            let our_peer_id = identity.public().to_peer_id();
            let rendezvous_nodes: Vec<RendezvousNode> = rendezvous_nodes
                .into_iter()
                .filter(|node| node.peer_id != our_peer_id)
                .collect();

            let backoffs = BackoffTracker::new(
                Self::RENDEZVOUS_RETRY_INITIAL_INTERVAL,
                Self::RENDEZVOUS_RETRY_MAX_INTERVAL,
                1.1f64,
            );

            let mut redial = redial::Behaviour::new(
                Self::REDIAL_IDENTIFIER,
                REDIAL_INITIAL_INTERVAL,
                REDIAL_MAX_INTERVAL,
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
                                    self.rendezvous_nodes[i].set_registration(
                                        RegistrationStatus::RegisterOnNextConnection,
                                    );
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
            self.inner.handle_established_outbound_connection(
                connection_id,
                peer,
                addr,
                role_override,
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

            let mut asb =
                new_swarm(|identity| super::Behaviour::new(identity, vec![rendezvous_point]));
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

            let mut asb =
                new_swarm(|identity| register::Behaviour::new(identity, rendezvous_nodes));
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
}

/// A behaviour that periodically discovers other peers at a given rendezvous point
///
/// The behaviour also internally attempts to dial any newly discovered peers
/// It uses the `redial` behaviour internally to do this
pub mod discovery {
    use backoff::{backoff::Backoff, ExponentialBackoff};
    use futures::future::{self};
    use futures::FutureExt;
    use libp2p::{
        identity, rendezvous,
        swarm::{NetworkBehaviour, THandlerInEvent, ToSwarm},
        Multiaddr, PeerId,
    };
    use std::{
        collections::{HashMap, HashSet, VecDeque},
        task::Poll,
        time::Duration,
    };

    use crate::{behaviour_util::{BackoffTracker, ConnectionTracker}, futures_util::FuturesHashSet, protocols::redial};

    static REDIAL_INITIAL_INTERVAL: Duration = Duration::from_secs(1);
    static REDIAL_MAX_INTERVAL: Duration = Duration::from_secs(10);

    // How to we retry failed discovery requests
    static DISCOVERY_INITIAL_INTERVAL: Duration = Duration::from_secs(1);
    static DISCOVERY_MAX_INTERVAL: Duration = Duration::from_secs(60 * 3);
    static DISCOVERY_MULTIPLIER: f64 = 1.25;

    // How often we wait after a successful discovery request to send another discovery request
    static DISCOVERY_INTERVAL: Duration = Duration::from_secs(10);

    pub struct Behaviour {
        inner: InnerBehaviour,
        namespace: rendezvous::Namespace,

        // Set of all (peer_id, address) pairs that we have discovered over the lifetime of the behaviour
        // This is also used to avoid notifying the swarm about the same pair multiple times
        discovered: HashSet<(PeerId, Multiaddr)>,

        // Track all the connections internally
        connection_tracker: ConnectionTracker,

        // Backoff for each rendezvous node for discovery
        backoff: BackoffTracker,

        // Queue of discovery requests to send to a rendezvous node
        // once the future resolves, the peer id is removed from the queue and a discovery request is sent
        pending_to_discover: FuturesHashSet<PeerId, ()>,

        // Queue of peers to send a request to as soon as we are connected to them
        to_discover: VecDeque<PeerId>,

        // Queue of events to be sent to the swarm
        to_swarm: VecDeque<ToSwarm<Event, THandlerInEvent<Self>>>,
    }

    #[derive(NetworkBehaviour)]
    pub struct InnerBehaviour {
        rendezvous: libp2p::rendezvous::client::Behaviour,
        redial: redial::Behaviour,
    }

    #[derive(Debug)]
    pub enum Event {
        DiscoveredPeer { peer_id: PeerId },
    }

    impl Behaviour {
        pub fn new(
            identity: identity::Keypair,
            rendezvous_nodes: Vec<PeerId>,
            namespace: rendezvous::Namespace,
        ) -> Self {
            let mut redial = redial::Behaviour::new(
                "rendezvous-server-for-discovery",
                REDIAL_INITIAL_INTERVAL,
                REDIAL_MAX_INTERVAL,
            );
            let rendezvous = libp2p::rendezvous::client::Behaviour::new(identity);

            let backoff = BackoffTracker::new(
                DISCOVERY_INITIAL_INTERVAL,
                DISCOVERY_MAX_INTERVAL,
                DISCOVERY_MULTIPLIER,
            );
            let mut to_discover = FuturesHashSet::new();

            // Initialize backoff for each rendezvous node
            for node in &rendezvous_nodes {
                // We initially schedule a discovery request for each rendezvous node
                to_discover.insert(node.clone(), future::ready(()).boxed());

                // We instruct the redial behaviour to dial rendezvous nodes periodically
                redial.add_peer(node.clone());
            }

            Self {
                inner: InnerBehaviour { rendezvous, redial },
                discovered: HashSet::new(),
                backoff,
                pending_to_discover: to_discover,
                to_discover: VecDeque::new(),
                connection_tracker: ConnectionTracker::new(),
                namespace,
                to_swarm: VecDeque::new(),
            }
        }
    }

    impl NetworkBehaviour for Behaviour {
        // We use a dummy connection handler here as we don't need low level connection handling
        // This is handled by the rendezvous behaviour
        type ConnectionHandler = <InnerBehaviour as NetworkBehaviour>::ConnectionHandler;

        type ToSwarm = Event;

        fn poll(
            &mut self,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<
            libp2p::swarm::ToSwarm<Self::ToSwarm, libp2p::swarm::THandlerInEvent<Self>>,
        > {
            loop {
                // Check if we have any events to send to the swarm
                if let Some(event) = self.to_swarm.pop_front() {
                    return Poll::Ready(event);
                }

                // Check if we should send a discovery request to a rendezvous node
                while let Poll::Ready(Some((peer_id, _))) =
                    self.pending_to_discover.poll_next_unpin(cx)
                {
                    self.to_discover.push_back(peer_id);
                }

                // Take ownership of the queue to avoid borrow checker issues
                let to_discover = std::mem::take(&mut self.to_discover);
                self.to_discover = to_discover
                    .into_iter()
                    .filter(|peer| {
                        // If we are not connected to the peer, keep it in the queue
                        if !self.connection_tracker.is_connected(peer) {
                            return true;
                        }

                        // If we are connected to the peer, send a discovery request
                        self.inner.rendezvous.discover(
                            Some(self.namespace.clone()),
                            None,
                            None,
                            peer.clone(),
                        );

                        false
                    })
                    .collect();

                match self.inner.poll(cx) {
                    Poll::Ready(ToSwarm::GenerateEvent(event)) => {
                        match event {
                            InnerBehaviourEvent::Rendezvous(
                                libp2p::rendezvous::client::Event::Discovered {
                                    rendezvous_node,
                                    registrations,
                                    ..
                                },
                            ) => {
                                tracing::trace!(
                                    ?rendezvous_node,
                                    num_registrations = %registrations.len(),
                                    "Discovered peers at rendezvous node"
                                );

                                for registration in registrations {
                                    for address in registration.record.addresses() {
                                        let peer_id = registration.record.peer_id();

                                        if self.discovered.insert((peer_id, address.clone())) {
                                            // TODO: Do we need to redial every peer we discover?
                                            self.inner
                                                .redial
                                                .add_peer_with_address(peer_id, address.clone());

                                            self.to_swarm.push_back(
                                                ToSwarm::NewExternalAddrOfPeer {
                                                    peer_id,
                                                    address: address.clone(),
                                                },
                                            );
                                            self.to_swarm.push_back(ToSwarm::GenerateEvent(
                                                Event::DiscoveredPeer { peer_id },
                                            ));
                                        }

                                        self.pending_to_discover.insert(
                                            rendezvous_node,
                                            tokio::time::sleep(DISCOVERY_INTERVAL).boxed(),
                                        );

                                        tracing::trace!(
                                            ?rendezvous_node,
                                            ?peer_id,
                                            ?address,
                                            "Discovered peer at rendezvous node"
                                        );
                                    }
                                }
                                continue;
                            }
                            InnerBehaviourEvent::Rendezvous(
                                libp2p::rendezvous::client::Event::DiscoverFailed {
                                    rendezvous_node,
                                    error,
                                    namespace: _,
                                },
                            ) => {
                                let backoff = self
                                    .backoff
                                    .get_backoff(&rendezvous_node)
                                    .next_backoff()
                                    .expect("backoff should never run out");

                                self.pending_to_discover
                                    .insert(rendezvous_node, tokio::time::sleep(backoff).boxed());

                                tracing::error!(
                                    ?rendezvous_node,
                                    ?error,
                                    seconds_until_next_discovery_attempt = %backoff.as_secs(),
                                    "Failed to discover peers at rendezvous node, scheduling retry after backoff"
                                );
                                continue;
                            }
                            _ => continue,
                        }
                    }
                    Poll::Ready(other) => {
                        self.to_swarm.push_back(other.map_out(|_| unreachable!()));
                        continue;
                    }
                    Poll::Pending => {
                        return Poll::Pending;
                    }
                }
            }
        }

        fn on_swarm_event(&mut self, event: libp2p::swarm::FromSwarm) {
            self.connection_tracker.handle_swarm_event(event);
            self.inner.on_swarm_event(event);
        }

        fn handle_established_inbound_connection(
            &mut self,
            connection_id: libp2p::swarm::ConnectionId,
            peer: PeerId,
            local_addr: &libp2p::Multiaddr,
            remote_addr: &libp2p::Multiaddr,
        ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
            self.inner.handle_established_inbound_connection(
                connection_id,
                peer,
                local_addr,
                remote_addr,
            )
        }

        fn handle_established_outbound_connection(
            &mut self,
            connection_id: libp2p::swarm::ConnectionId,
            peer: PeerId,
            addr: &libp2p::Multiaddr,
            role_override: libp2p::core::Endpoint,
        ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
            self.inner.handle_established_outbound_connection(
                connection_id,
                peer,
                addr,
                role_override,
            )
        }

        fn on_connection_handler_event(
            &mut self,
            peer_id: PeerId,
            connection_id: libp2p::swarm::ConnectionId,
            event: libp2p::swarm::THandlerOutEvent<Self>,
        ) {
            self.inner
                .on_connection_handler_event(peer_id, connection_id, event);
        }

        fn handle_pending_inbound_connection(
            &mut self,
            connection_id: libp2p::swarm::ConnectionId,
            local_addr: &Multiaddr,
            remote_addr: &Multiaddr,
        ) -> Result<(), libp2p::swarm::ConnectionDenied> {
            self.inner
                .handle_pending_inbound_connection(connection_id, local_addr, remote_addr)
        }

        fn handle_pending_outbound_connection(
            &mut self,
            connection_id: libp2p::swarm::ConnectionId,
            maybe_peer: Option<PeerId>,
            addresses: &[Multiaddr],
            effective_role: libp2p::core::Endpoint,
        ) -> Result<Vec<Multiaddr>, libp2p::swarm::ConnectionDenied> {
            self.inner.handle_pending_outbound_connection(
                connection_id,
                maybe_peer,
                addresses,
                effective_role,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::{new_swarm, SwarmExt};
    use futures::StreamExt;
    use libp2p::rendezvous;
    use libp2p::swarm::SwarmEvent;
    use std::sync::Once;
    use std::time::Duration;

    static INIT_TRACING: Once = Once::new();

    fn init_tracing() {
        INIT_TRACING.call_once(|| {
            let _ = tracing_subscriber::fmt()
                .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
                .try_init();
        });
    }

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
        init_tracing();

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
