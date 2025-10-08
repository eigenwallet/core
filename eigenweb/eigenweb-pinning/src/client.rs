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

use crate::{
    codec, futures_utils::FuturesHashSet, signature::SignedMessage, storage, PinRequest,
    PinResponse, PullRequest, SignedPinnedMessage, UnsignedPinnedMessage,
};

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
    to_swarm_events: VecDeque<Event>,

    /// We use this to persist data
    storage: S,

    /// Stores the backoff for each server
    backoff: HashMap<PeerId, backoff::ExponentialBackoff>,

    /// Hashes of all known messages we want to get pinned
    /// We only store the hash here.
    ///
    /// If we need the message (for example when we send it to someone),
    /// we can look it up in the storage.
    outgoing_messages: HashSet<[u8; 32]>,

    /// Hashes of messages we want to pull
    ///
    /// We only know the hash but do not have the full message.
    incoming_messages: HashSet<[u8; 32]>,

    /// For every server we store the set of hashes of messages that we know he has
    dont_want: HashMap<PeerId, Arc<HashSet<[u8; 32]>>>,

    /// Queues for outgoing requests
    ///
    /// These are futures as this allows us to schedule when they should be sent
    /// (for example after a backoff delay)
    queued_outgoing_pin_requests: FuturesHashSet<(PeerId, [u8; 32]), ()>,
    queued_outgoing_fetch_requests: FuturesHashSet<PeerId, ()>,
    queued_outgoing_pull_requests: FuturesHashSet<(PeerId, [u8; 32]), ()>,

    /// For each outbound request we make, we store the id in here
    /// TODO: Doesn't really do anything useful as of now
    inflight_pin_request: HashMap<OutboundRequestId, (PeerId, [u8; 32])>,
    inflight_pull_request: HashMap<OutboundRequestId, ()>,
    inflight_fetch_request: HashMap<OutboundRequestId, ()>,
}

#[derive(Debug)]
pub enum Event {
    PinRequestAcknowledged {
        peer: PeerId,
        hash: [u8; 32],
    },
    IncomingPinnedMessagesReceived {
        peer: PeerId,
        outgoing_request_id: OutboundRequestId,
        messages: Vec<SignedPinnedMessage>,
    },
}

impl<S: storage::Storage + 'static> Behaviour<S> {
    pub fn new(keypair: Keypair, servers: Vec<PeerId>, storage: S, timeout: Duration) -> Self {
        let peer_id = keypair.public().to_peer_id();
        let outgoing_messages_hashes: HashSet<_> =
            storage.hashes_by_sender(peer_id).into_iter().collect();

        Self {
            inner: codec::client(timeout),
            keypair,
            servers,
            storage,

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
                max_interval: Duration::from_secs(5 * 60),
                max_elapsed_time: None,
                ..ExponentialBackoff::default()
            })
    }

    fn mark_do_not_want(&mut self, peer_id: PeerId, hash: [u8; 32]) {
        let mut set = self
            .dont_want
            .entry(peer_id)
            .or_insert_with(|| Arc::new(HashSet::new()))
            .as_ref()
            .clone();
        set.insert(hash);
        self.dont_want.insert(peer_id, Arc::new(set));
    }

    fn dont_want_read_only(&self, peer_id: PeerId) -> Arc<HashSet<[u8; 32]>> {
        self.dont_want.get(&peer_id).cloned().unwrap_or_default()
    }

    /// Inserts a message into the internal system
    /// such that it will be contineously broadcasted
    pub fn insert_pinned_message(&mut self, message: UnsignedPinnedMessage) {
        // Sign the message
        let signed_message = SignedMessage::new(&self.keypair, message).unwrap();
        let message_hash = signed_message.content_hash();

        // Store in internal state
        self.outgoing_messages.insert(message_hash);
        self.mark_do_not_want(self.peer_id(), message_hash);

        // Save the message in storage
        self.storage.store(signed_message).unwrap();
    }

    /// Schedules a pin request for a server after backoff
    fn pin_at_server(&mut self, msg: SignedPinnedMessage) {
        let msg_hash = msg.content_hash();

        self.storage.store(msg.clone()).unwrap();
        self.outgoing_messages.insert(msg_hash);
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
                                self.mark_do_not_want(peer_id, hash);
                                self.backoff(peer_id).reset();

                                self.to_swarm_events
                                    .push_back(Event::PinRequestAcknowledged {
                                        peer: peer_id,
                                        hash,
                                    });
                            }
                        }
                        codec::Response::Fetch(fetch_response) => {
                            if let Some(_) = self.inflight_fetch_request.remove(&request_id) {
                                println!(
                                    "received fetch response from {} with data {:?}",
                                    peer, fetch_response
                                );

                                // These are the hashes the server has and he is claiming they are for us
                                let server_hashes: HashSet<_> =
                                    fetch_response.messages.into_iter().collect();

                                // Mark this server as having these hashes
                                // TODO: We replace here because fetch always returns all hashes
                                //       but we may want to extend here instead if logic changes
                                self.dont_want.insert(peer, Arc::new(server_hashes.clone()));

                                // Now we extend our [`incoming_messages`] set with the server's messages
                                self.incoming_messages.extend(server_hashes);
                            }
                        }
                        codec::Response::Pull(pull_response) => {
                            if let Some(_) = self.inflight_pull_request.remove(&request_id) {
                                let messages = pull_response
                                    .messages
                                    .into_iter()
                                    // Ensure the message is intended for us
                                    .filter(|message| message.message().receiver == self.peer_id())
                                    // Ensure the message is signed by the supposed sender
                                    .filter(|message| {
                                        message.verify_with_peer(message.message.sender)
                                    })
                                    .collect();

                                self.to_swarm_events.push_back(
                                    Event::IncomingPinnedMessagesReceived {
                                        peer,
                                        outgoing_request_id: request_id,
                                        messages,
                                    },
                                )
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            },
            libp2p::request_response::Event::OutboundFailure {
                request_id: _,
                peer: _,
                error: _,
            } => {
                todo!("handle network errors")
            }
            _ => {}
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
        loop {
            // Send pending pin requests
            while let Poll::Ready(Some(((peer_id, hash), _))) =
                self.queued_outgoing_pin_requests.poll_next_unpin(cx)
            {
                if let Some(message) = self.storage.get_by_hashes(vec![hash]).into_iter().next() {
                    let request = codec::Request::Pin(PinRequest { message });
                    let request_id = self.inner.send_request(&peer_id, request);
                    self.inflight_pin_request
                        .insert(request_id, (peer_id, hash));
                }
            }

            // Send pending fetch requests
            while let Poll::Ready(Some((peer_id, _))) =
                self.queued_outgoing_fetch_requests.poll_next_unpin(cx)
            {
                let request = codec::Request::Fetch(crate::FetchRequest {});
                let request_id = self.inner.send_request(&peer_id, request);
                self.inflight_fetch_request.insert(request_id, ());
            }

            // Send pending pull requests
            while let Poll::Ready(Some(((peer_id, hash), _))) =
                self.queued_outgoing_pull_requests.poll_next_unpin(cx)
            {
                let request = codec::Request::Pull(PullRequest { hashes: vec![hash] });
                let request_id = self.inner.send_request(&peer_id, request);
                self.inflight_pull_request.insert(request_id, ());
            }

            // Check if there is a server for which we do not have a [`dont_want`] set yet
            // this means we do not know which messages he has
            let servers = self.servers.clone();
            for server in servers.iter() {
                if !self.dont_want.contains_key(server)
                    && !self.queued_outgoing_fetch_requests.contains_key(server)
                {
                    println!("we do not have a [`dont_want`] set yet for {}, adding to pending outgoing fetch requests", server);

                    let backoff = self.backoff(*server).next_backoff().unwrap();
                    let future = async move {
                        tokio::time::sleep(backoff).await;
                    };
                    self.queued_outgoing_fetch_requests
                        .insert(*server, Box::pin(future));
                }
            }

            // For every server: see which hashes we want to send
            for server in servers.iter() {
                // All the hashes which the server does not have yet but we want him to have
                let dont_want = self.dont_want_read_only(*server);
                let hashes_to_send: HashSet<_> = self
                    .outgoing_messages
                    .difference(&dont_want)
                    .copied()
                    .collect();

                // Like hashes_to_send but excluding those which are already pending
                let inflight_hashes: HashSet<_> = self
                    .inflight_pin_request
                    .values()
                    .map(|(_, hash)| *hash)
                    .collect();
                let hashes_to_send_non_inflight = hashes_to_send.difference(&inflight_hashes);

                for hash in hashes_to_send_non_inflight {
                    if let Some(msg) = self.storage.get_by_hashes(vec![*hash]).into_iter().next() {
                        let request = codec::Request::Pin(PinRequest { message: msg });
                        let request_id = self.inner.send_request(&server, request);
                        self.inflight_pin_request
                            .insert(request_id, (*server, *hash));
                    }
                }
            }

            // For every `incoming_messages` check if we have them, otherwise pull them
            let our_hashes = self.dont_want_read_only(self.peer_id());
            let interesting_hashes: Vec<_> = self
                .incoming_messages
                .difference(&our_hashes)
                .copied()
                .collect();

            // For every interesting hash, attempt to download from all servers
            // for which we know they have it
            for hash in interesting_hashes {
                for server in servers.iter() {
                    if self.dont_want_read_only(*server).contains(&hash) {
                        let backoff = self.backoff(*server).next_backoff().unwrap();
                        let future = async move {
                            tokio::time::sleep(backoff).await;
                        };
                        self.queued_outgoing_pull_requests
                            .insert((*server, hash), Box::pin(future));
                    }
                }
            }

            match self.inner.poll(cx) {
                Poll::Ready(libp2p::swarm::ToSwarm::GenerateEvent(event)) => {
                    println!("received event from inner behaviour: {:?}", event);

                    if matches!(event, libp2p::request_response::Event::Message { .. }) {
                        self.handle_event(event);
                    }

                    continue;
                }
                Poll::Ready(other) => {
                    self.inner_events
                        .push_back(other.map_out(|_| unreachable!()));

                    continue;
                }
                Poll::Pending => {
                    break;
                }
            }
        }

        // TODO: Will be always be woken up here ? We might return while to_swarm_events is not empty
        if let Some(event) = self.inner_events.pop_front() {
            return Poll::Ready(event);
        }

        if let Some(event) = self.to_swarm_events.pop_front() {
            return Poll::Ready(ToSwarm::GenerateEvent(event));
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
