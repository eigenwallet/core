use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use anyhow::{Result, ensure};
use async_trait::async_trait;
use futures::FutureExt;
use futures::future::{BoxFuture, Fuse, FusedFuture, OptionFuture};
use libp2p::request_response::{self, OutboundFailure, OutboundRequestId, ProtocolSupport};
use libp2p::swarm::{
    ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, THandler, THandlerInEvent,
    THandlerOutEvent, ToSwarm,
};
use libp2p::{Multiaddr, PeerId, StreamProtocol};
use swap_machine::swap_attestation::{AttestedSwap, SwapAttestation, SwapTerms};
use uuid::Uuid;

use super::{PROTOCOL, Request, Response};
use crate::behaviour_util::BackoffTracker;
use crate::futures_util::FuturesHashSet;

/// Persists swap attestations and knows which swaps still need one.
#[async_trait]
pub trait SwapAttestationStore {
    /// Swaps that progressed at least to the Bitcoin being locked but have no attestation yet.
    async fn swaps_awaiting_attestation(&self) -> Result<Vec<SwapAwaitingAttestation>>;
    async fn store_swap_attestation(&self, attestation: SwapAttestation) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct SwapAwaitingAttestation {
    pub swap_id: Uuid,
    pub maker: PeerId,
    pub terms: SwapTerms,
}

pub struct Config {
    /// How often to look for swaps that still need an attestation.
    /// A swap whose request was rejected is requested again at the next poll.
    pub poll_interval: Duration,
    /// Backoff for retrying a request that failed on the network level.
    pub retry_initial_interval: Duration,
    pub retry_max_interval: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(5 * 60),
            retry_initial_interval: Duration::from_secs(30),
            retry_max_interval: Duration::from_secs(5 * 60),
        }
    }
}

type InnerBehaviour = request_response::cbor::Behaviour<Request, Response>;

/// Requests swap attestations from Alice for every swap that needs one, until each is stored.
pub struct Behaviour {
    inner: InnerBehaviour,
    local_peer_id: PeerId,
    store: Arc<dyn SwapAttestationStore + Send + Sync>,

    poll_interval: tokio::time::Interval,
    pending_query: OptionFuture<Fuse<BoxFuture<'static, Result<Vec<SwapAwaitingAttestation>>>>>,

    /// The swaps we are currently trying to get an attestation for, with the attestation we expect.
    tracked: HashMap<Uuid, AttestedSwap>,
    to_dispatch: VecDeque<Uuid>,
    inflight: HashMap<OutboundRequestId, Uuid>,
    retries: FuturesHashSet<Uuid, ()>,
    backoff: BackoffTracker<Uuid>,
    storing: FuturesHashSet<Uuid, Result<()>>,
}

impl Behaviour {
    pub fn new(
        local_peer_id: PeerId,
        store: Arc<dyn SwapAttestationStore + Send + Sync>,
        config: Config,
    ) -> Self {
        let mut poll_interval = tokio::time::interval(config.poll_interval);
        poll_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        Self {
            inner: InnerBehaviour::new(
                vec![(StreamProtocol::new(PROTOCOL), ProtocolSupport::Outbound)],
                request_response::Config::default()
                    .with_request_timeout(crate::defaults::DEFAULT_REQUEST_TIMEOUT),
            ),
            local_peer_id,
            store,
            poll_interval,
            pending_query: OptionFuture::from(None),
            tracked: HashMap::new(),
            to_dispatch: VecDeque::new(),
            inflight: HashMap::new(),
            retries: FuturesHashSet::new(),
            backoff: BackoffTracker::new(
                config.retry_initial_interval,
                config.retry_max_interval,
                crate::defaults::BACKOFF_MULTIPLIER,
            ),
            storing: FuturesHashSet::new(),
        }
    }

    fn track(&mut self, swaps: Vec<SwapAwaitingAttestation>) {
        for swap in swaps {
            if self.tracked.contains_key(&swap.swap_id) {
                continue;
            }

            let expected = AttestedSwap {
                maker: swap.maker,
                taker: self.local_peer_id,
                swap_id: swap.swap_id,
                terms: swap.terms,
            };
            self.tracked.insert(swap.swap_id, expected);
            self.to_dispatch.push_back(swap.swap_id);
        }
    }

    fn dispatch(&mut self, swap_id: Uuid) {
        let Some(expected) = self.tracked.get(&swap_id) else {
            return;
        };

        tracing::debug!(%swap_id, maker = %expected.maker, "Requesting swap attestation");
        let request_id = self
            .inner
            .send_request(&expected.maker, Request { swap_id });
        self.inflight.insert(request_id, swap_id);
    }

    fn handle_response(&mut self, request_id: OutboundRequestId, response: Response) {
        let Some(swap_id) = self.inflight.remove(&request_id) else {
            return;
        };
        self.backoff.remove(&swap_id);

        let attestation = match response {
            Response::Attested(attestation) => attestation,
            Response::Rejected(reason) => {
                tracing::info!(%swap_id, %reason, "Alice rejected swap attestation request, will request again later");
                self.tracked.remove(&swap_id);
                return;
            }
        };

        let Some(expected) = self.tracked.get(&swap_id) else {
            return;
        };
        if let Err(error) = check(expected, &attestation) {
            tracing::error!(%swap_id, ?error, "Alice sent an invalid swap attestation, will request again later");
            self.tracked.remove(&swap_id);
            return;
        }

        let store = Arc::clone(&self.store);
        self.storing.insert(
            swap_id,
            async move { store.store_swap_attestation(attestation).await }.boxed(),
        );
    }

    fn handle_failure(&mut self, request_id: OutboundRequestId, error: OutboundFailure) {
        let Some(swap_id) = self.inflight.remove(&request_id) else {
            return;
        };

        let delay = self.backoff.increment(&swap_id);
        tracing::debug!(%swap_id, %error, retry_in_secs = delay.as_secs(), "Failed to request swap attestation");
        self.retries
            .insert(swap_id, tokio::time::sleep(delay).boxed());
    }
}

fn check(expected: &AttestedSwap, attestation: &SwapAttestation) -> Result<()> {
    ensure!(
        attestation.swap == *expected,
        "Attested swap does not match our record of the swap"
    );
    attestation.verify()
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
        while let Poll::Ready(event) = self.inner.poll(cx) {
            let ToSwarm::GenerateEvent(event) = event else {
                return Poll::Ready(event.map_out(|_| unreachable!()));
            };

            match event {
                request_response::Event::Message {
                    message:
                        request_response::Message::Response {
                            request_id,
                            response,
                        },
                    ..
                } => self.handle_response(request_id, response),
                request_response::Event::OutboundFailure {
                    request_id, error, ..
                } => self.handle_failure(request_id, error),
                _ => {}
            }
        }

        if let Poll::Ready(Some(result)) = self.pending_query.poll_unpin(cx) {
            match result {
                Ok(swaps) => self.track(swaps),
                Err(error) => {
                    tracing::error!(?error, "Failed to load swaps awaiting an attestation")
                }
            }
        }

        if self.pending_query.is_terminated() && self.poll_interval.poll_tick(cx).is_ready() {
            let store = Arc::clone(&self.store);
            let query = async move { store.swaps_awaiting_attestation().await }
                .boxed()
                .fuse();
            self.pending_query = OptionFuture::from(Some(query));
            cx.waker().wake_by_ref();
        }

        while let Poll::Ready(Some((swap_id, ()))) = self.retries.poll_next_unpin(cx) {
            self.to_dispatch.push_back(swap_id);
        }

        while let Poll::Ready(Some((swap_id, result))) = self.storing.poll_next_unpin(cx) {
            self.tracked.remove(&swap_id);
            match result {
                Ok(()) => tracing::info!(%swap_id, "Stored swap attestation"),
                Err(error) => {
                    tracing::error!(%swap_id, ?error, "Failed to store swap attestation, will request again later")
                }
            }
        }

        if !self.to_dispatch.is_empty() {
            while let Some(swap_id) = self.to_dispatch.pop_front() {
                self.dispatch(swap_id);
            }
            cx.waker().wake_by_ref();
        }

        Poll::Pending
    }
}
