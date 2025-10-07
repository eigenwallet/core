use libp2p::{
    request_response::ResponseChannel,
    swarm::{FromSwarm, NetworkBehaviour},
};
use libp2p_identity::PeerId;
use std::{task::Poll, time::Duration};

use crate::{codec, storage, PinRejectReason, PinRequest, PinResponse};

pub struct Behaviour<S> {
    /// The inner request-response behaviour
    inner: crate::codec::Behaviour,

    /// We use this to persist data
    storage: S,
}

#[derive(Debug)]
pub struct Event {}

impl<S: storage::Storage + 'static> Behaviour<S> {
    pub fn new(storage: S, timeout: Duration) -> Self {
        Self {
            inner: crate::codec::server(timeout),
            storage,
        }
    }

    fn handle_pin_request(
        &mut self,
        request: PinRequest,
        peer: PeerId,
        channel: ResponseChannel<codec::Response>,
    ) {
        let unverified_signed_pinned_message = request.signed_msg;

        if !unverified_signed_pinned_message.verify_with_peer(peer) {
            // If the signature is invalid, reject immediately
            // TOOD: Ban the peer as he should not be relaying invalid signatures
            let _ = self.inner.send_response(
                channel,
                codec::Response::Pin(PinResponse::Rejected(PinRejectReason::MalformedMessage)),
            );

            return;
        }

        // Rename to make it clear that this has been verified
        let verified_signed_pinned_message = unverified_signed_pinned_message;

        // TODO: Use an async storage trait here; this will require more logic
        match self.storage.store(verified_signed_pinned_message) {
            Ok(_) => {
                let _ = self
                    .inner
                    .send_response(channel, codec::Response::Pin(PinResponse::Stored));
            }
            Err(_) => {
                // TODO: Log the error here
                let _ = self.inner.send_response(
                    channel,
                    codec::Response::Pin(PinResponse::Rejected(PinRejectReason::Other)),
                );
            }
        }
    }

    fn handle_pull_request(
        &mut self,
        _request: crate::PullRequest,
        peer: PeerId,
        channel: ResponseChannel<codec::Response>,
    ) {
        let messages = self.storage.retrieve(peer);
        let _ = self.inner.send_response(
            channel,
            codec::Response::Pull(crate::PullResponse { messages }),
        );
    }

    pub fn handle_event(&mut self, event: crate::codec::ToSwarm) {
        match event {
            libp2p::request_response::Event::Message { peer, message } => match message {
                libp2p::request_response::Message::Request {
                    request_id: _,
                    request,
                    channel,
                } => match request {
                    crate::codec::Request::Pin(request) => {
                        self.handle_pin_request(request, peer, channel);
                    }
                    crate::codec::Request::Pull(request) => {
                        self.handle_pull_request(request, peer, channel);
                    }
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
