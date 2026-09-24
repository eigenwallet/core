use std::sync::Arc;
use std::task::{Context, Poll};

use anyhow::Result;
use async_trait::async_trait;
use futures::FutureExt;
use futures::future::BoxFuture;
use futures::stream::{FuturesUnordered, StreamExt};
use libp2p::request_response::{self, ProtocolSupport, ResponseChannel};
use libp2p::swarm::{
    ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, THandler, THandlerInEvent,
    THandlerOutEvent, ToSwarm,
};
use libp2p::{Multiaddr, PeerId, StreamProtocol, identity};
use swap_machine::swap_attestation::{AttestedSwap, SwapAttestation, SwapTerms};
use uuid::Uuid;

use super::{PROTOCOL, Request, Response, SwapAttestationRejectReason};
use crate::protocols::metered::{Metered, RequestResponseMetrics};

/// Provides what Alice knows about a swap.
#[async_trait]
pub trait SwapAttestationSource {
    /// Returns `None` if the swap is unknown.
    async fn swap_attestation_record(&self, swap_id: Uuid) -> Result<Option<SwapRecord>>;
}

#[derive(Debug, Clone)]
pub struct SwapRecord {
    pub taker: PeerId,
    /// The terms of the swap, if the Bitcoin was locked.
    pub btc_locked_terms: Option<SwapTerms>,
}

type InnerBehaviour = Metered<request_response::cbor::Behaviour<Request, Response>>;

/// Answers swap attestation requests from Bob.
pub struct Behaviour {
    inner: InnerBehaviour,
    identity: identity::Keypair,
    source: Arc<dyn SwapAttestationSource + Send + Sync>,
    lookups: FuturesUnordered<BoxFuture<'static, Lookup>>,
}

struct Lookup {
    channel: ResponseChannel<Response>,
    peer: PeerId,
    swap_id: Uuid,
    record: Result<Option<SwapRecord>>,
}

impl Behaviour {
    pub fn new(
        identity: identity::Keypair,
        source: Arc<dyn SwapAttestationSource + Send + Sync>,
        metrics: Option<RequestResponseMetrics>,
    ) -> Self {
        Self {
            inner: Metered::new(
                request_response::cbor::Behaviour::new(
                    vec![(StreamProtocol::new(PROTOCOL), ProtocolSupport::Inbound)],
                    request_response::Config::default()
                        .with_request_timeout(crate::defaults::DEFAULT_REQUEST_TIMEOUT),
                ),
                PROTOCOL,
                metrics,
            ),
            identity,
            source,
            lookups: FuturesUnordered::new(),
        }
    }

    fn start_lookup(&mut self, peer: PeerId, swap_id: Uuid, channel: ResponseChannel<Response>) {
        let source = Arc::clone(&self.source);
        self.lookups.push(
            async move {
                let record = source.swap_attestation_record(swap_id).await;
                Lookup {
                    channel,
                    peer,
                    swap_id,
                    record,
                }
            }
            .boxed(),
        );
    }

    fn respond(&mut self, lookup: Lookup) {
        let Lookup {
            channel,
            peer,
            swap_id,
            record,
        } = lookup;

        let response = record.and_then(|record| self.response(peer, swap_id, record));
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                tracing::error!(%peer, %swap_id, ?error, "Failed to process swap attestation request");
                return;
            }
        };

        if let Response::Rejected(reason) = &response {
            tracing::info!(%peer, %swap_id, %reason, "Rejecting swap attestation request");
        }

        if self.inner.send_response(channel, response).is_err() {
            tracing::debug!(%peer, %swap_id, "Failed to send swap attestation response");
        }
    }

    fn response(
        &self,
        peer: PeerId,
        swap_id: Uuid,
        record: Option<SwapRecord>,
    ) -> Result<Response> {
        let Some(record) = record else {
            return Ok(Response::Rejected(SwapAttestationRejectReason::UnknownSwap));
        };
        if record.taker != peer {
            return Ok(Response::Rejected(
                SwapAttestationRejectReason::MaliciousRequest,
            ));
        }
        let Some(terms) = record.btc_locked_terms else {
            return Ok(Response::Rejected(
                SwapAttestationRejectReason::BtcNotLocked,
            ));
        };

        let swap = AttestedSwap {
            maker: self.identity.public().to_peer_id(),
            taker: peer,
            swap_id,
            terms,
        };

        Ok(Response::Attested(SwapAttestation::sign(
            swap,
            &self.identity,
        )?))
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = <InnerBehaviour as NetworkBehaviour>::ConnectionHandler;
    type ToSwarm = void::Void;

    fn handle_established_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer_id: PeerId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.inner.handle_established_inbound_connection(
            connection_id,
            peer_id,
            local_addr,
            remote_addr,
        )
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer_id: PeerId,
        addr: &Multiaddr,
        role_override: libp2p::core::Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.inner.handle_established_outbound_connection(
            connection_id,
            peer_id,
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
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        self.inner.handle_pending_outbound_connection(
            connection_id,
            maybe_peer,
            addresses,
            effective_role,
        )
    }

    fn on_swarm_event(&mut self, event: FromSwarm<'_>) {
        self.inner.on_swarm_event(event);
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        self.inner
            .on_connection_handler_event(peer_id, connection_id, event);
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        while let Poll::Ready(Some(lookup)) = self.lookups.poll_next_unpin(cx) {
            self.respond(lookup);
        }

        while let Poll::Ready(event) = self.inner.poll(cx) {
            let ToSwarm::GenerateEvent(event) = event else {
                return Poll::Ready(event.map_out(|_| unreachable!()));
            };

            if let request_response::Event::Message {
                peer,
                message:
                    request_response::Message::Request {
                        request, channel, ..
                    },
                ..
            } = event
            {
                self.start_lookup(peer, request.swap_id, channel);
                cx.waker().wake_by_ref();
            }
        }

        Poll::Pending
    }
}
