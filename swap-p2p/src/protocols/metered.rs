//! Per-protocol request-response metrics.
//!
//! [`Metered`] wraps any request-response [`NetworkBehaviour`] and counts the
//! messages it emits, labeled by protocol and kind, into a shared
//! [`RequestResponseMetrics`] family. It delegates everything else to the inner
//! behaviour (including, via [`Deref`], its inherent `send_request` /
//! `send_response` API).

use std::ops::{Deref, DerefMut};
use std::task::{Context, Poll};

use libp2p::PeerId;
use libp2p::core::{Endpoint, Multiaddr};
use libp2p::request_response::{Event, Message};
use libp2p::swarm::{
    ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, THandler, THandlerInEvent,
    THandlerOutEvent, ToSwarm,
};
use prometheus_client::encoding::EncodeLabelSet;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::registry::Registry;

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct Labels {
    protocol: String,
    kind: &'static str,
}

/// Counter family shared by all [`Metered`] behaviours. Cloning is cheap (the
/// family is reference-counted) and yields handles to the same counters.
#[derive(Clone, Debug)]
pub struct RequestResponseMetrics(Family<Labels, Counter>);

impl RequestResponseMetrics {
    pub fn register(registry: &mut Registry) -> Self {
        let family = Family::<Labels, Counter>::default();
        registry.register(
            "request_response_messages",
            "Request-response messages by protocol and kind",
            family.clone(),
        );
        Self(family)
    }

    fn record(&self, protocol: &'static str, kind: &'static str) {
        self.0
            .get_or_create(&Labels {
                protocol: protocol.to_string(),
                kind,
            })
            .inc();
    }
}

/// Wraps a request-response behaviour and records per-protocol message counts.
/// When `metrics` is `None`, it is a transparent passthrough.
#[allow(missing_debug_implementations)]
pub struct Metered<B> {
    inner: B,
    protocol: &'static str,
    metrics: Option<RequestResponseMetrics>,
}

impl<B> Metered<B> {
    pub fn new(inner: B, protocol: &'static str, metrics: Option<RequestResponseMetrics>) -> Self {
        Self {
            inner,
            protocol,
            metrics,
        }
    }

    fn record<Req, Resp>(&self, event: &Event<Req, Resp>) {
        let Some(metrics) = &self.metrics else {
            return;
        };

        let kind = match event {
            Event::Message {
                message: Message::Request { .. },
                ..
            } => "request",
            Event::Message {
                message: Message::Response { .. },
                ..
            } => "response",
            Event::ResponseSent { .. } => "response_sent",
            Event::InboundFailure { .. } => "inbound_failure",
            Event::OutboundFailure { .. } => "outbound_failure",
        };

        metrics.record(self.protocol, kind);
    }
}

impl<B> Deref for Metered<B> {
    type Target = B;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<B> DerefMut for Metered<B> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<B, Req, Resp> NetworkBehaviour for Metered<B>
where
    B: NetworkBehaviour<ToSwarm = Event<Req, Resp>>,
    Req: Send + 'static,
    Resp: Send + 'static,
{
    type ConnectionHandler = B::ConnectionHandler;
    type ToSwarm = B::ToSwarm;

    fn handle_pending_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<(), ConnectionDenied> {
        self.inner
            .handle_pending_inbound_connection(connection_id, local_addr, remote_addr)
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

    fn handle_pending_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        maybe_peer: Option<PeerId>,
        addresses: &[Multiaddr],
        effective_role: Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        self.inner.handle_pending_outbound_connection(
            connection_id,
            maybe_peer,
            addresses,
            effective_role,
        )
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        addr: &Multiaddr,
        role_override: Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.inner
            .handle_established_outbound_connection(connection_id, peer, addr, role_override)
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        self.inner.on_swarm_event(event)
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
        let event = match self.inner.poll(cx) {
            Poll::Ready(event) => event,
            Poll::Pending => return Poll::Pending,
        };

        if let ToSwarm::GenerateEvent(ref generated) = event {
            self.record(generated);
        }

        Poll::Ready(event)
    }
}
