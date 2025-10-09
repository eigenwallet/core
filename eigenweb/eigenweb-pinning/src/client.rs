use crate::{
    codec, futures_utils::FuturesHashSet, signature::{MessageHash, SignedMessage}, storage, PinRequest,
    PinResponse, PullRequest, SignedPinnedMessage, UnsignedPinnedMessage,
};
use backoff::{backoff::Backoff, ExponentialBackoff};
use libp2p::{
    request_response::OutboundRequestId,
    swarm::{FromSwarm, NetworkBehaviour, ToSwarm},
};
use libp2p_identity::{Keypair, PeerId};
use std::sync::Arc;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    task::Poll,
    time::Duration,
};
use tokio::sync::mpsc;

const HEARTBEAT_FETCH_INTERVAL: Duration = Duration::from_secs(5);

pub struct Behaviour<S> {
    /// The peer ID of the local node
    keypair: Keypair,

    /// The peer IDs of the servers
    servers: Vec<PeerId>,

    /// The inner request-response behaviour
    inner: codec::Behaviour,

    // the events from the inner behaviour which we will propagate to the swarm
    inner_events:
        VecDeque<libp2p::swarm::ToSwarm<Event, libp2p::swarm::THandlerInEvent<codec::Behaviour>>>,

    /// A queue of events to return to the swarm
    ///
    /// We can insert events anywhere and we will process them later in order when we are polled
    /// by the swarm.
    to_swarm_events:
        VecDeque<libp2p::swarm::ToSwarm<Event, libp2p::swarm::THandlerInEvent<codec::Behaviour>>>,

    /// We use this to persist data
    storage: S,

    /// Stores the backoff for each server
    backoff: HashMap<PeerId, backoff::ExponentialBackoff>,

    /// When we want to insert a message, into the internal system
    /// it is pushed into this channel
    message_queue_rx: mpsc::UnboundedReceiver<UnsignedPinnedMessage>,
    message_queue_tx: mpsc::UnboundedSender<UnsignedPinnedMessage>,

    /// Hashes of all known messages we want to get pinned
    ///
    /// If we need the message itself, we can look it up in the storage.
    outgoing_messages: HashSet<MessageHash>,

    /// Hashes of messages we want to pull
    ///
    /// We only know the hash but do not have the full message.
    incoming_messages: HashSet<MessageHash>,

    /// For every server we store the set of hashes of messages that we know he has
    dont_want: HashMap<PeerId, Arc<HashSet<MessageHash>>>,

    /// Queues for outgoing requests
    ///
    /// These are futures as this allows us to schedule when they should be sent
    queued_outgoing_pin_requests: FuturesHashSet<(PeerId, MessageHash), ()>,
    queued_outgoing_fetch_requests: FuturesHashSet<PeerId, ()>,
    queued_outgoing_pull_requests: FuturesHashSet<(PeerId, MessageHash), ()>,

    /// For each outbound request we make, we store the id in here
    /// TODO: Doesn't really do anything useful as of now
    inflight_pin_request: HashMap<OutboundRequestId, (PeerId, MessageHash)>,
    inflight_pull_request: HashMap<OutboundRequestId, PeerId>,
    inflight_fetch_request: HashMap<OutboundRequestId, PeerId>,
}

#[derive(Debug)]
pub enum Event {
    IncomingPinnedMessagesReceived {
        peer: PeerId,
        outgoing_request_id: OutboundRequestId,
        messages: Vec<SignedPinnedMessage>,
    },
}

// This is the API that will be publicly accessible
impl<S: storage::Storage + 'static> Behaviour<S> {
    /// Inserts a message into the internal system
    /// such that it will be contineously broadcasted
    pub fn pin_message(&mut self, message: UnsignedPinnedMessage) {
        self.message_queue_tx.send(message).unwrap();
    }
}

impl<S: storage::Storage + 'static> Behaviour<S> {
    pub fn new(keypair: Keypair, servers: Vec<PeerId>, storage: S, timeout: Duration) -> Self {
        let peer_id = keypair.public().to_peer_id();
        let outgoing_messages_hashes: HashSet<_> =
            storage.hashes_by_sender(peer_id).into_iter().collect();

        let (message_queue_tx, message_queue_rx) = mpsc::unbounded_channel();

        Self {
            inner: codec::client(timeout),
            keypair,
            servers,
            storage,

            message_queue_rx,
            message_queue_tx,

            inner_events: VecDeque::new(),
            to_swarm_events: VecDeque::new(),
            dont_want: HashMap::new(),
            backoff: HashMap::new(),

            outgoing_messages: outgoing_messages_hashes,
            incoming_messages: HashSet::new(),

            queued_outgoing_pin_requests: FuturesHashSet::new(),
            queued_outgoing_fetch_requests: FuturesHashSet::new(),
            queued_outgoing_pull_requests: FuturesHashSet::new(),

            inflight_pin_request: HashMap::new(),
            inflight_pull_request: HashMap::new(),
            inflight_fetch_request: HashMap::new(),
        }
    }

    fn peer_id(&self) -> PeerId {
        self.keypair.public().to_peer_id()
    }

    fn backoff(&mut self, peer_id: PeerId) -> &mut ExponentialBackoff {
        self.backoff
            .entry(peer_id)
            .or_insert_with(|| ExponentialBackoff {
                initial_interval: Duration::from_millis(50),
                current_interval: Duration::from_millis(50),
                max_interval: Duration::from_secs(5),
                max_elapsed_time: None,
                ..ExponentialBackoff::default()
            })
    }

    fn mark_do_not_want(&mut self, peer_id: PeerId, hash: MessageHash) {
        let mut set = self
            .dont_want
            .entry(peer_id)
            .or_insert_with(|| Arc::new(HashSet::new()))
            .as_ref()
            .clone();
        set.insert(hash);
        self.dont_want.insert(peer_id, Arc::new(set));
    }

    fn dont_want_read_only(&self, peer_id: PeerId) -> Option<Arc<HashSet<MessageHash>>> {
        self.dont_want.get(&peer_id).cloned()
    }

    fn schedule_backoff<T>(
        &mut self,
        peer_id: PeerId,
        value: T,
        wait: Duration, // we add this on top of the backoff
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send>>
    where
        T: Send + 'static,
    {
        let backoff = self.backoff(peer_id).next_backoff().unwrap() + wait;
        Box::pin(async move {
            tokio::time::sleep(backoff).await;
            value
        })
    }

    pub fn handle_event(&mut self, event: codec::ToSwarm) {
        match event {
            libp2p::request_response::Event::Message { peer, message } => match message {
                libp2p::request_response::Message::Response {
                    request_id,
                    response,
                } => {
                    match response {
                        codec::Response::Pin(PinResponse::Stored) => {
                            if let Some((peer_id, hash)) =
                                self.inflight_pin_request.remove(&request_id)
                            {
                                // we got a successful response, so we reset the backoff
                                self.backoff(peer).reset();

                                // we know the server has the message now
                                self.mark_do_not_want(peer_id, hash);
                            }
                        }
                        codec::Response::Fetch(fetch_response) => {
                            if let Some(peer) = self.inflight_fetch_request.remove(&request_id) {
                                // we got a successful response, so we reset the backoff
                                self.backoff(peer).reset();

                                // These are the hashes the server has and he is claiming they are for us
                                let server_hashes: HashSet<_> =
                                    fetch_response.messages.into_iter().collect();

                                // Mark this server as having these hashes
                                // TODO: We replace here because fetch always returns all hashes
                                //       but we may want to extend here instead if logic changes
                                self.dont_want.insert(peer, Arc::new(server_hashes.clone()));

                                // Now we extend our [`incoming_messages`] set with the server's messages
                                self.incoming_messages.extend(server_hashes.clone());

                                tracing::trace!("{} has {:?}", peer, server_hashes);
                            }
                        }
                        codec::Response::Pull(pull_response) => {
                            if let Some(peer) = self.inflight_pull_request.remove(&request_id) {
                                // we got a successful response, so we reset the backoff
                                self.backoff(peer).reset();

                                let messages: Vec<_> = pull_response
                                    .messages
                                    .into_iter()
                                    // Ensure the message is intended for us
                                    .filter(|message| message.message().receiver == self.peer_id())
                                    // Ensure the message is signed by the supposed sender
                                    .filter(|message| {
                                        message.verify_with_peer(message.message.sender)
                                    })
                                    .collect();

                                // Save all the hashes in our dont_want set
                                for message in messages.iter() {
                                    self.mark_do_not_want(self.peer_id(), message.content_hash());
                                }

                                self.to_swarm_events.push_back(ToSwarm::GenerateEvent(
                                    Event::IncomingPinnedMessagesReceived {
                                        peer,
                                        outgoing_request_id: request_id,
                                        messages,
                                    },
                                ))
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            },

            libp2p::request_response::Event::OutboundFailure {
                request_id,
                error,
                peer,
            } => {
                tracing::error!(
                    "Outbound failure for request {:?}: {:?} with peer {:?}",
                    request_id,
                    error,
                    peer
                );

                let _ = self.inflight_pin_request.remove(&request_id);
                let _ = self.inflight_pull_request.remove(&request_id);
                let _ = self.inflight_fetch_request.remove(&request_id);
            }
            libp2p::request_response::Event::InboundFailure {
                request_id,
                error,
                peer,
            } => {
                tracing::error!(
                    "Inbound failure for request {:?}: {:?} with peer {:?}",
                    request_id,
                    error,
                    peer
                );
            }
            other => {
                println!("Received event: {:?}", other);
            }
        }
    }
}

impl<S: storage::Storage + 'static> NetworkBehaviour for Behaviour<S> {
    type ConnectionHandler = <codec::Behaviour as NetworkBehaviour>::ConnectionHandler;

    type ToSwarm = Event;

    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<libp2p::swarm::ToSwarm<Self::ToSwarm, libp2p::swarm::THandlerInEvent<Self>>> {
        #[cfg(never)]
        {
            println!("========================");
            println!("We are getting polled!");
            println!("inner_events: {}", self.inner_events.len());
            println!("to_swarm_events: {}", self.to_swarm_events.len());
            println!(
                "queued_outgoing_pin_requests: {}",
                self.queued_outgoing_pin_requests.len()
            );
            println!(
                "queued_outgoing_fetch_requests: {}",
                self.queued_outgoing_fetch_requests.len()
            );
            println!(
                "queued_outgoing_pull_requests: {}",
                self.queued_outgoing_pull_requests.len()
            );
            println!("inflight_pin_request: {}", self.inflight_pin_request.len());
            println!(
                "inflight_pull_request: {}",
                self.inflight_pull_request.len()
            );
            println!(
                "inflight_fetch_request: {}",
                self.inflight_fetch_request.len()
            );
            println!("outgoing_messages: {}", self.outgoing_messages.len());
            println!("incoming_messages: {}", self.incoming_messages.len());
            println!();
            println!(
                "We know this many messages: {}",
                self.dont_want_read_only(self.peer_id())
                    .map(|s| s.len())
                    .unwrap_or(0)
            );
            println!("========================");
        }

        // Move messages from the message queue to the internal system
        while let Poll::Ready(Some(message)) = self.message_queue_rx.poll_recv(cx) {
            // Sign the message
            let signed_message = SignedMessage::new(&self.keypair, message).unwrap();
            let message_hash = signed_message.content_hash();

            // Store in internal state
            self.outgoing_messages.insert(message_hash);
            self.mark_do_not_want(self.peer_id(), message_hash);

            // Save the message in storage
            self.storage.store(signed_message).unwrap();
        }

        // Send pending pin requests
        while let Poll::Ready(Some(((peer_id, hash), _))) =
            self.queued_outgoing_pin_requests.poll_next_unpin(cx)
        {
            let message = self
                .storage
                .get_by_hashes(vec![hash])
                .into_iter()
                .next()
                .unwrap();
            let request = codec::Request::Pin(PinRequest { message });
            let request_id = self.inner.send_request(&peer_id, request);
            self.inflight_pin_request
                .insert(request_id, (peer_id, hash));

            tracing::debug!("Pinning {:?} at {}", hash, peer_id);
        }

        // Send pending fetch requests
        while let Poll::Ready(Some((peer_id, _))) =
            self.queued_outgoing_fetch_requests.poll_next_unpin(cx)
        {
            let request = codec::Request::Fetch(crate::FetchRequest {});
            let request_id = self.inner.send_request(&peer_id, request);
            self.inflight_fetch_request.insert(request_id, peer_id);

            tracing::debug!("Fetching from {}", peer_id);
        }

        // Send pending pull requests
        while let Poll::Ready(Some(((peer_id, hash), _))) =
            self.queued_outgoing_pull_requests.poll_next_unpin(cx)
        {
            let request = codec::Request::Pull(PullRequest { hashes: vec![hash] });
            let request_id = self.inner.send_request(&peer_id, request);
            self.inflight_pull_request.insert(request_id, peer_id);

            tracing::debug!("Pulling {} from {}", hash, peer_id);
        }

        // Send an initial fetch request for ever server for
        // which we don't have a [`dont_want`] set yet
        // (This means we do not know which messages he has)
        //
        // If we already have a [`dont_want`] set but no pending queued entry, we add one with the heartbeat interval
        // (We know what messages he had in the past but that may have changed)
        {
            let servers = self.servers.clone();
            let servers_to_fetch: Vec<_> = servers
                .iter()
                .filter(|server| {
                    !self.queued_outgoing_fetch_requests.contains_key(server)
                })
                .copied()
                .map(|server| if self.dont_want.contains_key(&server) {
                    (server, HEARTBEAT_FETCH_INTERVAL)
                } else {
                    (server, Duration::ZERO)
                })

                .collect();

            for (server, interval) in servers_to_fetch {
                tracing::info!(
                    "We have no `dont_want` set for {} (or the heartbeat interval is due)",
                    server
                );

                let future = self.schedule_backoff(server, (), interval);
                self.queued_outgoing_fetch_requests.insert(server, future);
            }
        }

        // Pin messages to server which where we know:
        // - they do not have the message
        // - we do not have an inflight pin request for the message
        for server in self.servers.clone().iter() {
            let dont_want = match self.dont_want_read_only(*server) {
                Some(dont_want) => dont_want,
                // We only pin a message when we know which messages the server has
                None => continue,
            };

            let inflight_hashes: HashSet<_> = self
                .inflight_pin_request
                .values()
                .filter(|(peer, _)| *peer == *server)
                .map(|(_, hash)| *hash)
                .collect();

            let hashes_to_send: Vec<_> = self
                .outgoing_messages
                .iter()
                .filter(|hash| !dont_want.contains(*hash))
                .filter(|hash| !inflight_hashes.contains(*hash))
                .copied()
                .collect();

            if hashes_to_send.is_empty() {
                continue;
            }

            for hash in hashes_to_send {
                let future = self.schedule_backoff(*server, (), Duration::ZERO);
                tracing::debug!(
                    "Queuing {:?} to be pinned at {}",
                    hash,
                    server
                );
                self.queued_outgoing_pin_requests
                    .insert((*server, hash), future);
            }
        }

        // For every `incoming_messages` check if we have them, otherwise pull them
        let our_hashes = self.dont_want_read_only(self.peer_id());
        let interesting_hashes: Vec<_> = self
            .incoming_messages
            .difference(&our_hashes.unwrap_or_default())
            .copied()
            .collect();

        // For every interesting hash, attempt to download from all servers
        // for which we know they have it
        //
        // TODO: This is not very efficient as we will download the same message from multiple servers
        for hash in interesting_hashes {
            let servers = self.servers.clone();
            for server in servers.iter() {
                if let Some(dont_want) = self.dont_want_read_only(*server) {
                    if dont_want.contains(&hash) {
                        tracing::info!("We are interested in {:?}, pulling from {}", hash, server);

                        let future = self.schedule_backoff(*server, (), Duration::ZERO);
                        self.queued_outgoing_pull_requests
                            .insert((*server, hash), future);
                    }
                }
            }
        }

        while let Poll::Ready(event) = self.inner.poll(cx) {
            match event {
                ToSwarm::GenerateEvent(inner_event) => {
                    tracing::trace!("Received event from inner behaviour: {:?}", inner_event);
                    self.handle_event(inner_event);
                }
                other => {
                    self.inner_events
                        .push_back(other.map_out(|_| unreachable!()));
                }
            }
        }

        if let Some(event) = self.inner_events.pop_front() {
            return Poll::Ready(event);
        }

        if let Some(event) = self.to_swarm_events.pop_front() {
            return Poll::Ready(event);
        }

        Poll::Pending
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
        self.inner
            .handle_established_outbound_connection(connection_id, peer, addr, role_override)
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        self.inner.on_swarm_event(event)
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        connection_id: libp2p::swarm::ConnectionId,
        event: libp2p::swarm::THandlerOutEvent<Self>,
    ) {
        self.inner
            .on_connection_handler_event(peer_id, connection_id, event)
    }
}
