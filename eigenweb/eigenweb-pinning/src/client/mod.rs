use crate::{
    codec, fetch, pin, pull,
    signature::{MessageHash, SignedMessage},
    storage, SignedPinnedMessage, UnsignedPinnedMessage,
};
use libp2p::{
    request_response::{Message, OutboundRequestId},
    swarm::{FromSwarm, NetworkBehaviour, ToSwarm},
};
use libp2p_identity::{Keypair, PeerId};
use std::sync::Arc;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    task::Poll,
    time::Duration,
};
use swap_p2p::{futures_util::FuturesHashSet, protocols::redial};
use tokio::sync::mpsc;

mod backoff_pool;

const HEARTBEAT_FETCH_INTERVAL: Duration = Duration::from_secs(5);
const BACKOFF_INITIAL_INTERVAL: Duration = Duration::from_millis(50);
const BACKOFF_MAX_INTERVAL: Duration = Duration::from_secs(5);

pub struct Behaviour<S: storage::Storage + Sync> {
    /// The peer ID of the local node
    keypair: Keypair,

    /// The peer IDs of the servers
    servers: Vec<PeerId>,

    /// The inner request-response behaviour
    inner: InnerBehaviour,

    // the events from the inner behaviour which we will propagate to the swarm
    inner_events:
        VecDeque<libp2p::swarm::ToSwarm<Event, libp2p::swarm::THandlerInEvent<InnerBehaviour>>>,

    /// A queue of events to return to the swarm
    ///
    /// We can insert events anywhere and we will process them later in order when we are polled
    /// by the swarm.
    to_swarm_events:
        VecDeque<libp2p::swarm::ToSwarm<Event, libp2p::swarm::THandlerInEvent<InnerBehaviour>>>,

    /// We use this to persist data
    storage: Arc<S>,

    /// Stores the backoff for each peer, as to not bombard them with requests
    backoff: backoff_pool::Pool<BackoffKind>,

    /// Stores the backoff for each message hash, individual delays for specific messages?
    /// TODO: Is this needed?
    // message_hash_backoff: HashMap<MessageHash, backoff::ExponentialBackoff>,

    /// When we want to insert a message, into the internal system
    /// it is pushed into this channel
    message_queue_rx: mpsc::UnboundedReceiver<UnsignedPinnedMessage>,
    message_queue_tx: mpsc::UnboundedSender<UnsignedPinnedMessage>,

    /// Hashes of all known messages we want to get pinned
    ///
    /// If we need the message itself, we can look it up in the storage.
    outgoing_messages: HashSet<MessageHash>,

    // TODO: Combine `incoming_messages` and `outgoing_messages` into a single set with a boolean flag for incoming/outgoing?
    /// Hashes of messages we want to pull
    ///
    /// We only know the hash but do not have the full message.
    incoming_messages: HashSet<MessageHash>,

    /// For every server we store the set of hashes of messages that we know he has
    // TODO: Maybe extract this to a separate struct given the ton of helper functions we have for it?
    dont_want: HashMap<PeerId, Arc<HashSet<MessageHash>>>,

    // TODO: We also need to store which hashes a server **cannot* provide
    //       this means we have tried pulling
    //       We are already kind of doing this via our backoff pool?
    /// Queues for outgoing requests
    ///
    /// These are futures as this allows us to schedule when they should be sent
    ///
    /// TODO: This is basically a tokio::DelayQueue but with an additional HashMap
    queued_outgoing_pin_requests: FuturesHashSet<(PeerId, MessageHash), ()>,
    queued_outgoing_fetch_requests: FuturesHashSet<PeerId, ()>,
    queued_outgoing_pull_requests: FuturesHashSet<(PeerId, MessageHash), ()>,

    /// For each outbound request we make, we store the id in here
    /// This gives us a way to detect if we have a request inflight for a given peer (and hash if applicable)
    /// TODO: We iterate over the values a lot more than over the keys, maybe we should invert this or use another data structure?
    inflight_pin_request: HashMap<OutboundRequestId, (PeerId, MessageHash)>,
    inflight_pull_request: HashMap<OutboundRequestId, (PeerId, MessageHash)>,
    inflight_fetch_request: HashMap<OutboundRequestId, PeerId>,

    pending_storage_store: FuturesHashSet<MessageHash, Result<(), S::Error>>,

    /// When the event loop wants to get a certain message, it calls storage.get(...) and insert the future into `pending_storage_get`
    /// once that future completes, the result is put into `completed_storage_get`.
    pending_storage_get: FuturesHashSet<MessageHash, Result<Option<SignedPinnedMessage>, S::Error>>,

    // todo: this should be able to hold multiple values per hash? or it should cache values using a ringbuffer ?
    completed_storage_get: HashMap<MessageHash, Result<Option<SignedPinnedMessage>, S::Error>>,
}

#[derive(PartialEq, Eq, Hash, Debug, Copy, Clone)]
pub enum BackoffKind {
    Fetch,
    Pull(MessageHash),
    Pin(MessageHash),
}

#[derive(Debug)]
pub enum Event {
    IncomingPinnedMessageReceived(MessageHash),
}

/// Internal Behaviour that does two things:
/// 1. Handles request-response interactions with the servers
/// 2. Handles redialing of the servers
#[derive(NetworkBehaviour)]
pub struct InnerBehaviour {
    codec: codec::Behaviour,
    redial: redial::Behaviour,
}

impl InnerBehaviour {
    const REDIAL_NAME: &'static str = "pinning-servers";

    fn new(
        request_response_timeout: Duration,
        redial_interval: Duration,
        redial_max_interval: Duration,
        servers: Vec<PeerId>,
    ) -> Self {
        let mut redial =
            redial::Behaviour::new(Self::REDIAL_NAME, redial_interval, redial_max_interval);
        for server in servers.iter() {
            redial.add_peer(*server);
        }

        Self {
            codec: codec::client(request_response_timeout),
            redial,
        }
    }
}

// This is the API that will be publicly accessible
impl<S: storage::Storage + Sync + 'static> Behaviour<S> {
    /// Inserts a message into the internal system
    /// such that it will be contineously broadcasted
    pub fn pin_message(&mut self, message: UnsignedPinnedMessage) {
        self.message_queue_tx.send(message).unwrap();
    }
}

impl<S: storage::Storage + Sync + 'static> Behaviour<S> {
    pub async fn new(
        keypair: Keypair,
        servers: Vec<PeerId>,
        storage: Arc<S>,
        timeout: Duration,
    ) -> Self {
        let (message_queue_tx, message_queue_rx) = mpsc::unbounded_channel();

        // Populate the outgoing messages set with the messages from the storage layer
        // TODO: Maybe there is a cleaner way to do this?
        let peer_id = keypair.public().to_peer_id();
        let outgoing_messages_hashes: HashSet<_> = storage
            .hashes_by_sender(peer_id)
            .await
            .into_iter()
            .collect();

        Self {
            inner: InnerBehaviour::new(
                timeout,
                BACKOFF_INITIAL_INTERVAL,
                BACKOFF_MAX_INTERVAL,
                servers.clone(),
            ),
            keypair,
            servers,
            storage,

            message_queue_rx,
            message_queue_tx,

            inner_events: VecDeque::new(),
            to_swarm_events: VecDeque::new(),
            dont_want: HashMap::new(),
            backoff: backoff_pool::Pool::new(BACKOFF_INITIAL_INTERVAL, BACKOFF_MAX_INTERVAL),

            outgoing_messages: outgoing_messages_hashes,
            incoming_messages: HashSet::new(),

            queued_outgoing_pin_requests: FuturesHashSet::new(),
            queued_outgoing_fetch_requests: FuturesHashSet::new(),
            queued_outgoing_pull_requests: FuturesHashSet::new(),

            inflight_pin_request: HashMap::new(),
            inflight_pull_request: HashMap::new(),
            inflight_fetch_request: HashMap::new(),

            pending_storage_store: FuturesHashSet::new(),
            pending_storage_get: FuturesHashSet::new(),

            completed_storage_get: HashMap::new(),
        }
    }

    /// Gives us our own peer ID
    fn peer_id(&self) -> PeerId {
        self.keypair.public().to_peer_id()
    }

    fn dont_want_set(&mut self, peer_id: PeerId) -> &mut Arc<HashSet<MessageHash>> {
        self.dont_want
            .entry(peer_id)
            .or_insert_with(|| Arc::new(HashSet::new()))
    }

    fn mark_does_not_want(&mut self, peer_id: PeerId, hash: MessageHash) {
        let mut set = self
            .dont_want_set(peer_id)
            .as_ref()
            // TODO: This clone is expensive!
            .clone();

        set.insert(hash);

        self.dont_want.insert(peer_id, Arc::new(set));
    }

    /// Call this whenever we definitively know a server does not have a message
    fn mark_does_not_have(&mut self, peer_id: PeerId, hash: MessageHash) {
        let mut set = self.dont_want_set(peer_id).as_ref().clone();
        set.remove(&hash);
        self.dont_want.insert(peer_id, Arc::new(set));
    }

    fn dont_want_read_only(&self, peer_id: PeerId) -> Option<Arc<HashSet<MessageHash>>> {
        self.dont_want.get(&peer_id).cloned()
    }

    /// Ensures we have a pending store operation for the message
    ///
    /// Returns true if a new operation was queued, false if a operation was already pending
    fn queue_store_message(&mut self, message: SignedPinnedMessage) -> bool {
        // Immediately check if there is already a pending store for the message to avoid cloning the message
        if self
            .pending_storage_store
            .contains_key(&message.content_hash())
        {
            return false;
        }

        // We immediately mark the message as not wanted to avoid pulling it again
        // We will remove it from our `dont_want` set again if we fail to store it
        self.mark_does_not_want(self.peer_id(), message.content_hash());

        let storage = self.storage.clone();
        let message_clone = message.clone();

        self.pending_storage_store.insert(
            message.content_hash(),
            Box::pin(async move { storage.pin(message_clone).await }),
        )
    }

    /// Ensures we have a pending get operation for the hash
    ///
    /// Returns true if a new operation was queued, false if a operation was already pending
    fn queue_get_message(&mut self, hash: MessageHash) -> bool {
        if self.pending_storage_get.contains_key(&hash) {
            return false;
        }

        let storage = self.storage.clone();
        self.pending_storage_get.insert(
            hash,
            Box::pin(async move { storage.get_by_hash(hash).await }),
        )
    }

    pub fn handle_codec_event(&mut self, event: codec::Event) {
        match event {
            codec::Event::Message { peer, message } => match message {
                Message::Response {
                    request_id,
                    response,
                } => {
                    match response {
                        // Handle Pin responses
                        codec::Response::Pin(Ok(pin::Response::Stored)) => {
                            if let Some((peer, hash)) =
                                self.inflight_pin_request.remove(&request_id)
                            {
                                self.backoff.reset_backoff(peer, BackoffKind::Pin(hash));
                                self.mark_does_not_want(peer, hash);
                            }
                        }
                        codec::Response::Pin(Err(pin_error)) => {
                            if let Some((peer_id, hash)) =
                                self.inflight_pin_request.remove(&request_id)
                            {
                                self.backoff.increase_backoff(peer, BackoffKind::Pin(hash));

                                tracing::warn!(
                                    ?pin_error,
                                    ?peer_id,
                                    ?hash,
                                    "Server responded to our pin request with an error"
                                );
                            }
                        }
                        // Handle Fetch responses
                        codec::Response::Fetch(Ok(fetch::Response { incoming, outgoing })) => {
                            if let Some(peer) = self.inflight_fetch_request.remove(&request_id) {
                                self.backoff.reset_backoff(peer, BackoffKind::Fetch);

                                let incoming: HashSet<_> = incoming.into_iter().collect();
                                let outgoing: HashSet<_> = outgoing.into_iter().collect();

                                tracing::trace!(%peer, ?incoming, ?outgoing, "Server told us which hashes it has after fetch");

                                // Now we extend our [`incoming_messages`] set with the servers `incoming` messages
                                self.incoming_messages.extend(incoming.clone());

                                // The server just told us which hashes it has.
                                // We can therefore deduce that he is not interested in them anymore.
                                //
                                // TODO: We replace here because fetch always returns all hashes
                                //       but we may want to extend here instead if logic changes
                                // TODO: Redundant clone here?
                                let mut incoming_and_outgoing = HashSet::new();
                                incoming_and_outgoing.extend(incoming);
                                incoming_and_outgoing.extend(outgoing);
                                self.dont_want.insert(peer, Arc::new(incoming_and_outgoing));
                            }
                        }
                        codec::Response::Fetch(Err(fetch_error)) => {
                            if let Some(peer) = self.inflight_fetch_request.remove(&request_id) {
                                self.backoff.increase_backoff(peer, BackoffKind::Fetch);

                                tracing::warn!(
                                    ?fetch_error,
                                    ?peer,
                                    "Server responded to our fetch request with an error"
                                );
                            }
                        }
                        // Handle Pull responses
                        codec::Response::Pull(Ok(pull::Response { messages })) => {
                            // TODO: This should be able to pull multiple messages at once
                            if let Some((peer, hash)) =
                                self.inflight_pull_request.remove(&request_id)
                            {
                                self.backoff.reset_backoff(peer, BackoffKind::Pull(hash));

                                let messages: Vec<_> = messages
                                    .into_iter()
                                    // Ensure the message is intended for us
                                    .filter(|message| message.message().receiver == self.peer_id())
                                    // Ensure the message is signed by the supposed sender
                                    .filter(|message| {
                                        // TODO: Ban the peer if the signature is invalid as they should not be relaying invalid signatures
                                        message.verify_with_peer(message.message.sender)
                                    })
                                    .collect();

                                assert!(
                                    messages.len() == 1,
                                    "Pull response should contain exactly one message (for now)"
                                );

                                // If the list does not contain the hash we asked for,
                                // it means that the server cannot provide the hash.
                                //
                                // We therefore mark the server as not having the message
                                //
                                // TODO: Possible infinite loop triggered somewhere here?
                                if messages
                                    .iter()
                                    .find(|message| message.content_hash() == hash)
                                    .is_none()
                                {
                                    self.mark_does_not_have(peer, hash);
                                }

                                // Save all the hashes in the storage layer
                                for message in messages.iter() {
                                    // We know the server has this message because we just pulled it from him
                                    self.mark_does_not_want(peer, message.content_hash());

                                    // Queue the message to be stored
                                    self.queue_store_message(message.clone());
                                }
                            }
                        }
                        codec::Response::Pull(Err(pull_error)) => {
                            if let Some((peer, hash)) =
                                self.inflight_pull_request.remove(&request_id)
                            {
                                self.backoff.increase_backoff(peer, BackoffKind::Pull(hash));

                                tracing::warn!(
                                    ?pull_error,
                                    ?peer,
                                    ?hash,
                                    "Server responded to our pull request with an error"
                                );
                            }
                        }
                    }
                }
                _ => {}
            },
            codec::Event::OutboundFailure {
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

                // Increase the respective backoffs for the failed request
                if let Some((peer, hash)) = self.inflight_pin_request.remove(&request_id) {
                    tracing::warn!(
                        ?peer,
                        ?hash,
                        ?error,
                        ?request_id,
                        "Outbound failure for pin request, increasing backoff"
                    );

                    self.backoff.increase_backoff(peer, BackoffKind::Pin(hash));
                }

                if let Some((peer, hash)) = self.inflight_pull_request.remove(&request_id) {
                    tracing::warn!(
                        ?peer,
                        ?hash,
                        ?error,
                        ?request_id,
                        "Outbound failure for pull request, increasing backoff"
                    );

                    self.backoff.increase_backoff(peer, BackoffKind::Pull(hash));
                }

                if let Some(peer) = self.inflight_fetch_request.remove(&request_id) {
                    tracing::warn!(
                        ?peer,
                        ?error,
                        ?request_id,
                        "Outbound failure for fetch request, increasing backoff"
                    );

                    self.backoff.increase_backoff(peer, BackoffKind::Fetch);
                }
            }
            codec::Event::InboundFailure {
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

    pub fn handle_redial_event(&mut self, event: redial::Event) {
        match event {
            redial::Event::ScheduledRedial { peer, next_dial_in } => {
                tracing::trace!(%peer, next_dial_in_secs = %next_dial_in.as_secs(), "Scheduling redial for server");
            }
        }
    }
}

impl<S: storage::Storage + Sync + 'static> NetworkBehaviour for Behaviour<S> {
    type ConnectionHandler = <InnerBehaviour as NetworkBehaviour>::ConnectionHandler;

    type ToSwarm = Event;

    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<libp2p::swarm::ToSwarm<Self::ToSwarm, libp2p::swarm::THandlerInEvent<Self>>> {
        // Poll the pending storage futures
        {
            while let Poll::Ready(Some((hash, store_result))) =
                self.pending_storage_store.poll_next_unpin(cx)
            {
                match store_result {
                    Ok(()) => {
                        // We just stored the message, so we do not want it anymore
                        self.mark_does_not_want(self.peer_id(), hash);

                        // Inform the swarm that we just stored a new message
                        self.to_swarm_events.push_back(ToSwarm::GenerateEvent(
                            Event::IncomingPinnedMessageReceived(hash),
                        ));
                    }
                    Err(_) => {
                        // We failed to store the message, so we do not have it and we want it again

                        // TODO: Handle errors by potentially retrying
                        // TODO: This can lead to an infinite loop where we keep fetching the same message over and over again
                        // from the same server without backing off.
                        // Queue pull operation -> Do pull operation -> Fail to store message -> Queue pull operation ...
                        self.mark_does_not_have(self.peer_id(), hash);
                    }
                }
            }

            while let Poll::Ready(Some((hash, result))) =
                self.pending_storage_get.poll_next_unpin(cx)
            {
                self.completed_storage_get.insert(hash, result);
            }
        }

        // Move messages from the message queue to the internal system
        while let Poll::Ready(Some(message)) = self.message_queue_rx.poll_recv(cx) {
            // TODO: Extract this to a function

            // Sign the message
            let signed_message = SignedMessage::new(&self.keypair, message).unwrap();
            let message_hash = signed_message.content_hash();

            // Store in internal state
            self.outgoing_messages.insert(message_hash);
            self.mark_does_not_want(self.peer_id(), message_hash);

            // Save the message in storage
            self.queue_store_message(signed_message);
        }

        // Send pending pin requests
        while let Poll::Ready(Some(((peer_id, hash), _))) =
            self.queued_outgoing_pin_requests.poll_next_unpin(cx)
        {
            // Check if we have the hash ready from the storage layer
            if let Some(message) = self.completed_storage_get.remove(&hash) {
                // TODO: Do not unwrap here
                let message = message.unwrap().unwrap();

                let request = codec::Request::Pin(pin::Request { message });
                let outbound_request_id = self.inner.codec.send_request(&peer_id, request);
                self.inflight_pin_request
                    .insert(outbound_request_id, (peer_id, hash));

                tracing::debug!(%peer_id, %hash, %outbound_request_id, "Dispatching pin request");
            } else {
                // If we do not have the hash ready from the storage layer,
                // ensure we have a pending get operation for the storage
                self.queue_get_message(hash);
            }
        }

        // Send pending fetch requests
        while let Poll::Ready(Some((peer_id, _))) =
            self.queued_outgoing_fetch_requests.poll_next_unpin(cx)
        {
            let request = codec::Request::Fetch(fetch::Request);
            let outbound_request_id = self.inner.codec.send_request(&peer_id, request);
            self.inflight_fetch_request
                .insert(outbound_request_id, peer_id);

            tracing::debug!(%peer_id, %outbound_request_id, "Dispatching fetch request");
        }

        // Send pending pull requests
        while let Poll::Ready(Some(((peer_id, hash), _))) =
            self.queued_outgoing_pull_requests.poll_next_unpin(cx)
        {
            let request = codec::Request::Pull(pull::Request { hashes: vec![hash] });
            let outbound_request_id = self.inner.codec.send_request(&peer_id, request);
            self.inflight_pull_request
                .insert(outbound_request_id, (peer_id, hash));

            tracing::debug!(%peer_id, %hash, %outbound_request_id, "Dispatching pull request");
        }

        // Queue all queable fetch requests
        for (peer_id, has_dont_want_set) in self.queable_fetch_requests() {
            // TODO:
            // If the hearbeat interval is pretty long (which it will probably be in production), and we want to queue a fetch request as soon as possible, it will be difficult
            // because only a single queued entry is possible at a time.
            //
            // Let's say the hearbeat interval is 5m and we want to fetch now. Then we will not be able to insert a new entry into the queue until 5m have passed.
            // Even if we try to add a new queue entry, it will "fail silently" because the queue already has an entry for that peer. This is not optimal.
            // The queue would need a way to know **how long** the current entry will take to resolve and then replace it if the new entry would resolve sooner.
            // It is almost impossible to even insert anything into the queue because the hearbeat will be inserted as soon as possible and cannot be replaced.
            // Maybe the future unordered is too generic here?
            let wait_time = if has_dont_want_set {
                HEARTBEAT_FETCH_INTERVAL
            } else {
                Duration::ZERO
            };

            let (future, wait_time) =
                self.backoff
                    .schedule_backoff(peer_id, (), BackoffKind::Fetch, wait_time);

            self.queued_outgoing_fetch_requests.insert(peer_id, future);

            match has_dont_want_set {
                false => {
                    tracing::trace!(%peer_id, wait_time_secs = %wait_time.as_secs(), "Queued fetch request because we have no `dont_want` set");
                }
                true => {
                    tracing::trace!(%peer_id, wait_time_secs = %wait_time.as_secs(), "Queued fetch request with heart interval");
                }
            }
        }

        // Queue all queable pin messages
        for (peer_id, hash) in self.queable_pin_requests() {
            let (future, wait_time) =
                self.backoff
                    .schedule_backoff(peer_id, (), BackoffKind::Pin(hash), Duration::ZERO);

            self.queued_outgoing_pin_requests
                .insert((peer_id, hash), future);

            tracing::debug!(%peer_id, %hash, wait_time_secs = %wait_time.as_secs(), "Queued pin request");
        }

        // Queue all queable pull requests
        for (peer_id, hash) in self.queable_pull_requests() {
            let (future, wait_time) =
                self.backoff
                    .schedule_backoff(peer_id, (), BackoffKind::Pull(hash), Duration::ZERO);

            self.queued_outgoing_pull_requests
                .insert((peer_id, hash), future);

            tracing::debug!(%peer_id, %hash, wait_time_secs = %wait_time.as_secs(), "Queued pull request");
        }

        while let Poll::Ready(event) = self.inner.poll(cx) {
            match event {
                // We nest this to ensure we map every `GenerateEvent` variant
                ToSwarm::GenerateEvent(event) => match event {
                    InnerBehaviourEvent::Codec(inner_event) => {
                        self.handle_codec_event(inner_event);
                    }
                    InnerBehaviourEvent::Redial(inner_event) => {
                        self.handle_redial_event(inner_event);
                    }
                },
                other => {
                    self.inner_events.push_back(
                        other.map_out(|_| unreachable!("we manually map `GenerateEvent` variants")),
                    );
                }
            }
        }

        // TODO: Are these popped in the correct order?
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

/// Logic to determine which requests should be queued
impl<S: storage::Storage + Sync + 'static> Behaviour<S> {
    /// Returns a list of (server, message_hash) pairs that should be queued for pinning
    ///
    /// A message will be included if:
    /// - The server doesn't already have it (not in their dont_want set)
    /// - There is no inflight pin request for this message to this server
    /// - There is no queued pin request for this message to this server
    fn queable_pin_requests(&self) -> Vec<(PeerId, MessageHash)> {
        let mut result = Vec::new();

        for server in self.servers.clone().iter() {
            // Ensure we have a dont_want set for the server
            // We only pin a message when we know which messages the server has and can be sure he does not have it
            let Some(dont_want) = self.dont_want_read_only(*server) else {
                continue;
            };

            // Which hashes have we already dispatched to be pinned at this server?
            // This means we have sent the request but not yet received a response
            let inflight_hashes: HashSet<_> = self
                .inflight_pin_request
                .values()
                .filter(|(peer, _)| *peer == *server)
                .map(|(_, hash)| *hash)
                .collect();

            let hashes_to_send: Vec<_> = self
                .outgoing_messages
                .iter()
                // Ensure the server does not have the message
                .filter(|hash| !dont_want.contains(*hash))
                // Ensure we have not already dispatched the message to this server
                .filter(|hash| !inflight_hashes.contains(*hash))
                // Ensure we have not already queued the message to be pinned to this server
                .filter(|hash| {
                    !self
                        .queued_outgoing_pin_requests
                        .contains_key(&(*server, **hash))
                })
                .copied()
                .collect();

            for hash in hashes_to_send {
                result.push((*server, hash));
            }
        }

        result
    }

    /// Returns a list of (server, message_hash) pairs that should be queued for pulling
    ///
    /// A message will be included if:
    /// - We know about the message but don't have it yet (present in `incoming_messages` but not in our `dont_want` set)
    /// - The server does have the message (present in their `dont_want` set)
    /// - There is no inflight pull request for this message from this server
    /// - There is no queued pull request for this message from this server
    fn queable_pull_requests(&self) -> Vec<(PeerId, MessageHash)> {
        let mut result = Vec::new();

        // For every `incoming_messages` check if we have them, otherwise pull them
        let our_hashes = self.dont_want_read_only(self.peer_id());
        let interesting_hashes: Vec<_> = self
            .incoming_messages
            .difference(our_hashes.as_ref().unwrap_or(&Default::default()))
            .copied()
            .collect();

        // For every interesting hash, attempt to download from all servers
        // for which we know they have it
        //
        // TODO: This is not very efficient as we will download the same message from multiple servers
        for hash in interesting_hashes {
            for server in self.servers.iter() {
                // We ignore servers for which we cannot know if they have the message (no `dont_want` set)
                let Some(dont_want) = self.dont_want_read_only(*server) else {
                    continue;
                };

                // Does the server have the message?
                let has_message = dont_want.contains(&hash);

                // Have we already dispatched a pull request for this message to this server?
                let is_inflight = self
                    .inflight_pull_request
                    .values()
                    .any(|v| v == &(*server, hash));

                // Have we already queued a pull request for this message to this server?
                let is_queued = self
                    .queued_outgoing_pull_requests
                    .contains_key(&(*server, hash));

                if has_message && !is_inflight && !is_queued {
                    result.push((*server, hash));
                }
            }
        }

        result
    }

    /// Returns a list of (server, has_dont_want_set) pairs that should be queued for fetching
    ///
    /// A server will be included if:
    /// - We have not already dispatched a fetch request for this server
    /// - We have not already queued a fetch request for this server
    ///
    /// has_dont_want_set is true if we have a `dont_want` set for the server, meaning we already did an initial fetch
    fn queable_fetch_requests(&self) -> Vec<(PeerId, bool)> {
        let servers = self.servers.clone();

        let result: Vec<_> = servers
            .iter()
            // Ensure we have not already dispatched a fetch request for this server
            .filter(|server| {
                !self
                    .inflight_fetch_request
                    .values()
                    .any(|peer| *peer == **server)
            })
            // Ensure we have not already queued a fetch request for this server
            .filter(|server| !self.queued_outgoing_fetch_requests.contains_key(server))
            .copied()
            .map(|server| (server, self.dont_want.contains_key(&server)))
            .collect();

        result
    }
}
