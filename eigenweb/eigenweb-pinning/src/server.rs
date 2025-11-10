/// This file is much less complete than client.rs
use libp2p::{
    futures::{stream::FuturesUnordered, FutureExt},
    request_response::ResponseChannel,
    swarm::{FromSwarm, NetworkBehaviour},
};
use libp2p_identity::PeerId;
use std::{sync::Arc, task::Poll, time::Duration};

use crate::{codec, fetch, signature::MessageHash, storage, SignedPinnedMessage};

pub type ToSwarm = libp2p::swarm::ToSwarm<Event, libp2p::swarm::THandlerInEvent<codec::Behaviour>>;

pub struct Behaviour<S: storage::Storage> {
    /// The inner request-response behaviour
    inner: codec::Behaviour,

    /// We use this to persist data
    storage: Arc<S>,

    /// When we receive a request we usually need to do some async work on storage layer to craft a response.
    ///
    /// We cannot do this directly in the behaviour as we cannot call .await inside a poll() method (duh)
    ///
    /// We therefore call the async function and insert the returns future into one of the queues along with the response channel.
    /// Once the future completes, we look at the result from the storage layer. Then craft and send the response to the peer.
    inflight_pin: FuturesUnordered<
        std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = (Result<(), S::Error>, ResponseChannel<codec::Response>),
                    > + Send,
            >,
        >,
    >,
    inflight_pull: FuturesUnordered<
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
    inflight_fetch: FuturesUnordered<
        std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = (
                            // First vector is for incoming messages, second vector is for outgoing messages
                            // of the requester peer
                            Result<(Vec<MessageHash>, Vec<MessageHash>), S::Error>,
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

            inflight_pin: FuturesUnordered::new(),
            inflight_pull: FuturesUnordered::new(),
            inflight_fetch: FuturesUnordered::new(),
        }
    }

    /// Helper function to enqueue a storage task with its response channel.
    /// Takes a closure that receives the storage Arc and returns a future.
    fn enqueue_storage_task<T, F, Fut>(
        storage: Arc<S>,
        queue: &mut FuturesUnordered<
            std::pin::Pin<
                Box<dyn std::future::Future<Output = (T, ResponseChannel<codec::Response>)> + Send>,
            >,
        >,
        f: F,
        channel: ResponseChannel<codec::Response>,
    ) where
        T: Send + 'static,
        F: FnOnce(Arc<S>) -> Fut,
        Fut: std::future::Future<Output = T> + Send + 'static,
    {
        let future = f(storage);
        queue.push(Box::pin(future.map(|result| (result, channel))));
    }

    fn handle_pin_request(
        &mut self,
        request: crate::pin::Request,
        peer: PeerId,
        channel: ResponseChannel<codec::Response>,
    ) {
        if !request.message.verify_with_peer(peer) {
            // If the signature is invalid, reject immediately
            // TODO: Ban the peer as he should not be relaying invalid signatures
            let _ = self.inner.send_response(
                channel,
                codec::Response::Pin(Err(crate::pin::Error::MalformedMessage)),
            );
            return;
        }

        Self::enqueue_storage_task(
            Arc::clone(&self.storage),
            &mut self.inflight_pin,
            |storage| async move { storage.pin(request.message).await },
            channel,
        );
    }

    fn handle_pull_request(
        &mut self,
        request: crate::pull::Request,
        peer: PeerId,
        channel: ResponseChannel<codec::Response>,
    ) {
        Self::enqueue_storage_task(
            Arc::clone(&self.storage),
            &mut self.inflight_pull,
            |storage| async move { storage.get_by_receiver_and_hash(peer, request.hashes).await },
            channel,
        );
    }

    fn handle_fetch_request(
        &mut self,
        _request: crate::fetch::Request,
        peer: PeerId,
        channel: ResponseChannel<codec::Response>,
    ) {
        Self::enqueue_storage_task(
            Arc::clone(&self.storage),
            &mut self.inflight_fetch,
            |storage| async move { storage.get_hashes_involving(peer).await },
            channel,
        );
    }

    pub fn handle_event(&mut self, event: codec::Event) {
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

        // Respond to inflight pin requests where the storage layer returned a result
        while let Poll::Ready(Some((result, channel))) = self.inflight_pin.poll_next_unpin(cx) {
            let response = match result {
                Ok(()) => codec::Response::Pin(Ok(crate::pin::Response::Stored)),
                Err(err) => {
                    tracing::warn!(?err, "Storage layer returned an error while responding to a pin request. We will reject the request and notify the peer.");
                    codec::Response::Pin(Err(crate::pin::Error::Other))
                }
            };

            if let Err(err) = self.inner.send_response(channel, response) {
                tracing::warn!(?err, "Failed to send response to a pin request");
            }
        }

        // Respond to inflight pull requests where the storage layer returned a result
        while let Poll::Ready(Some((result, channel))) = self.inflight_pull.poll_next_unpin(cx) {
            let response = match result {
                Ok(messages) => codec::Response::Pull(Ok(crate::pull::Response { messages })),
                Err(err) => {
                    tracing::warn!(
                        ?err,
                        "Storage layer returned an error while responding to a pull request"
                    );
                    codec::Response::Pull(Err(crate::pull::Error::StorageFailure))
                }
            };

            if let Err(err) = self.inner.send_response(channel, response) {
                tracing::warn!(?err, "Failed to send response to a pull request");
            }
        }

        // Respond to inflight fetch requests where the storage layer returned a result
        while let Poll::Ready(Some((result, channel))) = self.inflight_fetch.poll_next_unpin(cx) {
            let response = match result {
                Ok((incoming, outgoing)) => {
                    codec::Response::Fetch(Ok(fetch::Response { incoming, outgoing }))
                }
                Err(err) => {
                    tracing::warn!(
                        ?err,
                        "Storage layer returned an error while responding to a fetch request"
                    );
                    codec::Response::Fetch(Err(fetch::Error::StorageFailure))
                }
            };

            if let Err(err) = self.inner.send_response(channel, response) {
                tracing::warn!(?err, "Failed to send response to a fetch request");
            }
        }

        // Poll the inner request-response behaviour
        //
        // We use `while let` to ensure we drain all ToSwarm events in one go.
        while let Poll::Ready(event) = self.inner.poll(cx) {
            match event {
                // Handle ToSwarm events from the inner behaviour ourselves in the `handle_event` method
                // Swallow them by not forwarding them to the swarm. These events are not interesting to any behaviours above us.
                libp2p::swarm::ToSwarm::GenerateEvent(event) => {
                    self.handle_event(event);
                }
                // Forward all other (non-GenerateEvent) ToSwarm variants to the swarm directly.
                // These include commands like Dial, ListenOn, etc. that must reach the swarm.
                // Returning Poll::Ready(...) causes the swarm to continue its internal loop,
                // which may poll us again in the same tick after processing this event.
                event => {
                    return Poll::Ready(event.map_out(|_| {
                        unreachable!(
                            "we manually map `GenerateEvent` variants in the match arm above"
                        )
                    }))
                }
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
        // We forward all swarm events to the inner behaviour.
        // We have no use for these currently so we don't process them ourselves.
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
