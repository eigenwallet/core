
use libp2p::{
    request_response::OutboundRequestId,
    swarm::{FromSwarm, NetworkBehaviour},
};
use libp2p_identity::PeerId;
use std::{collections::{HashMap, HashSet}, task::Poll, time::Duration};

use crate::{PinRequest, PinResponse, SignedPinnedMessage, codec, storage};

pub struct Behaviour<S> {
    /// The inner request-response behaviour
    inner: codec::Behaviour,

    /// We use this to persist data
    storage: S,

    /// Interval for the heartbeat
    heartbeat_interval: tokio::time::Interval,

    /// Hashes of all known messages we want to get pinned
    /// We only store the hash here, if we need the message
    /// we can look it up in the storage
    outgoing_messages_hashes: HashSet<[u8; 32]>,

    /// For every server we store the set of hashes
    /// that we know he already has pinned
    dont_want: HashMap<PeerId, HashSet<[u8; 32]>>,

    /// Stores the associated message hash for an inflight request to pin
    inflight_request_to_pin: HashMap<(OutboundRequestId, PeerId), [u8; 32]>,
}

#[derive(Debug)]
pub struct Event {}

impl<S: storage::Storage + 'static> Behaviour<S> {
    pub fn new(peer_id: PeerId, storage: S, timeout: Duration) -> Self {
        let outgoing_messages_hashes: HashSet<_> = storage
            .hashes_by_sender(peer_id)
            .into_iter()
            .collect();

        let dont_want = HashMap::new();

        Self {
            inner: codec::client(timeout),
            storage,
            outgoing_messages_hashes,
            dont_want,
            inflight_request_to_pin: HashMap::new(),
            heartbeat_interval: tokio::time::interval(timeout),
        }
    }

    pub fn pin_message(&mut self, message: SignedPinnedMessage) {
        self.outgoing_messages_hashes.insert(message.content_hash());
        self.storage.store(message).unwrap();
    }

    fn send_pin_request(&mut self, peer_id: PeerId, hash: [u8; 32]) {
        let message = self.storage.get_by_hash(hash);

        if let Some(signed_msg) = message {
            let request = codec::Request::Pin(PinRequest {
                signed_msg,
            });

            let request_id = self.inner.send_request(&peer_id, request);
            self.inflight_request_to_pin.insert((request_id, peer_id), hash);
        } else {
            todo!("handle this")
        }
    }

    fn heartbeat(&mut self) {
        let to_send: Vec<_> = self.dont_want
            .iter()
            .flat_map(|(peer, hashes)| {
                self.outgoing_messages_hashes
                    .difference(hashes)
                    .map(move |hash| (*peer, *hash))
            })
            .collect();

        for (peer, hash) in to_send {
            self.send_pin_request(peer, hash);
        }
    }

    pub fn handle_event(&mut self, event: crate::codec::ToSwarm) {
        match event {
            libp2p::request_response::Event::Message { peer, message } => match message {
                libp2p::request_response::Message::Response { request_id, response } => match response {
                    codec::Response::Pin(PinResponse::Stored) => {
                        if let Some(hash) = self.inflight_request_to_pin.remove(&(request_id, peer)) {
                            self.dont_want.entry(peer).or_insert_with(HashSet::new).insert(hash);
                        }
                    }
                    codec::Response::Pull(_) => {}
                    _ => {}
                },
                _ => {}
            },
            _ => {}
        }
    }
}

impl<S: storage::Storage + 'static> NetworkBehaviour for Behaviour<S> {
    type ConnectionHandler = <crate::codec::Behaviour as NetworkBehaviour>::ConnectionHandler;

    type ToSwarm = Event;

    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<libp2p::swarm::ToSwarm<Self::ToSwarm, libp2p::swarm::THandlerInEvent<Self>>> {
        if self.heartbeat_interval.poll_tick(cx).is_ready() {
            self.heartbeat();
        }

        // Is this the correct way to handle events?
        // Do we need an unreachable!() here?
        match self.inner.poll(cx) {
            Poll::Ready(libp2p::swarm::ToSwarm::GenerateEvent(event)) => {
                if matches!(
                    event,
                    libp2p::request_response::Event::Message {
                        message: libp2p::request_response::Message::Request { .. },
                        ..
                    }
                ) {
                    self.handle_event(event);
                    Poll::Pending
                } else {
                    Poll::Ready(libp2p::swarm::ToSwarm::GenerateEvent(Event {}))
                }
            }
            Poll::Ready(other) => Poll::Ready(other.map_out(|_| unreachable!())),
            _ => Poll::Pending,
        }
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
