/// This file is much less complete than client.rs
use libp2p::{
    futures::stream::FuturesUnordered,
    request_response::ResponseChannel,
    swarm::{FromSwarm, NetworkBehaviour},
};
use libp2p_identity::PeerId;
use std::{collections::VecDeque, sync::Arc, task::Poll, time::Duration};

use crate::{
    codec, signature::MessageHash, storage, PinRejectReason, PinRequest, PinResponse,
    SignedPinnedMessage,
};

pub type ToSwarm = libp2p::swarm::ToSwarm<Event, libp2p::swarm::THandlerInEvent<codec::Behaviour>>;

pub struct Behaviour<S: storage::Storage> {
    /// The inner request-response behaviour
    inner: codec::Behaviour,

    to_swarm: VecDeque<ToSwarm>,

    /// We use this to persist data
    storage: Arc<S>,

    pending_storage_pin: FuturesUnordered<
        std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = (Result<(), S::Error>, ResponseChannel<codec::Response>),
                    > + Send,
            >,
        >,
    >,
    pending_storage_pull: FuturesUnordered<
        std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = (
                            Result<Vec<SignedPinnedMessage>, S::Error>,
                            ResponseChannel<codec::Response>,
                        ),
                    > + Send,
            >,
        >,
    >,
    pending_storage_fetch: FuturesUnordered<
        std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = (
                            Result<Vec<MessageHash>, S::Error>,
                            ResponseChannel<codec::Response>,
                        ),
                    > + Send,
            >,
        >,
    >,
}

#[derive(Debug)]
pub struct Event {}

impl<S: storage::Storage + Sync + 'static> Behaviour<S> {
    pub fn new(storage: S, timeout: Duration) -> Self {
        Self {
            inner: codec::server(timeout),
            storage: Arc::new(storage),
            to_swarm: VecDeque::new(),

            pending_storage_pin: FuturesUnordered::new(),
            pending_storage_pull: FuturesUnordered::new(),
            pending_storage_fetch: FuturesUnordered::new(),
        }
    }

    fn handle_pin_request(
        &mut self,
        request: PinRequest,
        peer: PeerId,
        channel: ResponseChannel<codec::Response>,
    ) {
        if !request.message.verify_with_peer(peer) {
            // If the signature is invalid, reject immediately
            // TODO: Ban the peer as he should not be relaying invalid signatures
            let _ = self.inner.send_response(
                channel,
                codec::Response::Pin(PinResponse::Rejected(PinRejectReason::MalformedMessage)),
            );
            return;
        }

        let storage = self.storage.clone();
        self.pending_storage_pin.push(Box::pin(async move {
            let result = storage.pin(request.message).await;
            (result, channel)
        }));
    }

    fn handle_pull_request(
        &mut self,
        request: crate::PullRequest,
        peer: PeerId,
        channel: ResponseChannel<codec::Response>,
    ) {
        let storage = self.storage.clone();
        let hashes = request.hashes;
        self.pending_storage_pull.push(Box::pin(async move {
            let result = storage.get_by_receiver_and_hash(peer, hashes).await;
            (result, channel)
        }));
    }

    fn handle_fetch_request(
        &mut self,
        _request: crate::FetchRequest,
        peer: PeerId,
        channel: ResponseChannel<codec::Response>,
    ) {
        let storage = self.storage.clone();
        self.pending_storage_fetch.push(Box::pin(async move {
            let result = storage.get_hashes_involving(peer).await;
            (result, channel)
        }));
    }

    pub fn handle_event(&mut self, event: codec::ToSwarm) {
        match event {
            libp2p::request_response::Event::Message { peer, message } => match message {
                libp2p::request_response::Message::Request {
                    request_id: _,
                    request,
                    channel,
                } => match request {
                    codec::Request::Pin(request) => {
                        self.handle_pin_request(request, peer, channel);
                    }
                    codec::Request::Pull(request) => {
                        self.handle_pull_request(request, peer, channel);
                    }
                    codec::Request::Fetch(request) => {
                        self.handle_fetch_request(request, peer, channel);
                    }
                },
                _ => {}
            },
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
            }
            _ => {}
        }
    }
}

impl<S: storage::Storage + Sync + 'static> NetworkBehaviour for Behaviour<S> {
    type ConnectionHandler = <codec::Behaviour as NetworkBehaviour>::ConnectionHandler;

    type ToSwarm = Event;

    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<libp2p::swarm::ToSwarm<Self::ToSwarm, libp2p::swarm::THandlerInEvent<Self>>> {
        use libp2p::futures::StreamExt;

        // Poll the pending storage futures
        {
            while let Poll::Ready(Some((result, channel))) =
                self.pending_storage_pin.poll_next_unpin(cx)
            {
                if result.is_ok() {
                    let _ = self
                        .inner
                        .send_response(channel, codec::Response::Pin(PinResponse::Stored));
                } else {
                    // TODO: Log the error here
                    let _ = self.inner.send_response(
                        channel,
                        codec::Response::Pin(PinResponse::Rejected(PinRejectReason::Other)),
                    );
                }
            }

            while let Poll::Ready(Some((result, channel))) =
                self.pending_storage_pull.poll_next_unpin(cx)
            {
                match result {
                    Ok(messages) => {
                        let _ = self.inner.send_response(
                            channel,
                            codec::Response::Pull(crate::PullResponse { messages }),
                        );
                    }
                    Err(_) => {
                        // TODO: Log the error here
                        let _ = self.inner.send_response(
                            channel,
                            codec::Response::Pull(crate::PullResponse { messages: vec![] }),
                        );
                    }
                }
            }

            while let Poll::Ready(Some((result, channel))) =
                self.pending_storage_fetch.poll_next_unpin(cx)
            {
                match result {
                    Ok(messages) => {
                        let _ = self.inner.send_response(
                            channel,
                            codec::Response::Fetch(crate::FetchResponse { messages }),
                        );
                    }
                    Err(_) => {
                        // TODO: Log the error here
                        let _ = self.inner.send_response(
                            channel,
                            codec::Response::Fetch(crate::FetchResponse { messages: vec![] }),
                        );
                    }
                }
            }
        }

        // TODO: Is this the correct way to handle events?
        // TODO: Will this always wake us up again ?
        while let Poll::Ready(event) = self.inner.poll(cx) {
            match event {
                libp2p::swarm::ToSwarm::GenerateEvent(event) => {
                    self.handle_event(event);
                }
                // Do we need an unreachable!() here?
                event => self.to_swarm.push_back(event.map_out(|_| unreachable!())),
            }
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
