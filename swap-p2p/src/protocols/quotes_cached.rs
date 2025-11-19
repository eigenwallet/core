use crate::futures_util::FuturesHashSet;
use crate::protocols::quote::BidQuote;
use crate::protocols::quotes;
use libp2p::core::ConnectedPoint;
use libp2p::swarm::{
    ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, THandler, THandlerInEvent,
    THandlerOutEvent, ToSwarm,
};
use libp2p::{Multiaddr, PeerId};
use std::collections::{HashMap, VecDeque};
use std::task::{Context, Poll};

use crate::out_event;

pub struct Behaviour {
    inner: quotes::Behaviour,

    // For each peer, cache the address we last connected to them at
    addresses: HashMap<PeerId, Multiaddr>,

    // Caches quotes
    cache: HashMap<PeerId, BidQuote>,
    expiry: FuturesHashSet<PeerId, ()>,

    // Queue of events to be sent to the swarm
    to_swarm: VecDeque<Event>,
}

impl Behaviour {
    pub fn new() -> Self {
        Self {
            inner: quotes::Behaviour::new(),
            addresses: HashMap::new(),
            cache: HashMap::new(),
            expiry: FuturesHashSet::new(),
            to_swarm: VecDeque::new(),
        }
    }

    fn emit_cached_quotes(&mut self) {
        // Attach the address we last connected to the peer at to the quote
        // Ignores those peers where we don't have an address cached (should never happen)
        let quotes = self
            .cache
            .iter()
            .filter_map(|(peer_id, quote)| {
                self.addresses
                    .get(peer_id)
                    .map(|addr| (*peer_id, addr.clone(), quote.clone()))
            })
            .collect();

        self.to_swarm.push_back(Event::CachedQuotes { quotes });
    }
}

#[derive(Debug)]
pub enum Event {
    // TODO: This should somehow also send a addr with this
    CachedQuotes {
        quotes: Vec<(PeerId, Multiaddr, BidQuote)>,
    },
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = <quotes::Behaviour as NetworkBehaviour>::ConnectionHandler;
    type ToSwarm = Event;

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        while let Poll::Ready(Some((peer, ()))) = self.expiry.poll_next_unpin(cx) {
            self.cache.remove(&peer);
            self.emit_cached_quotes();
        }

        while let Poll::Ready(event) = self.inner.poll(cx) {
            match event {
                ToSwarm::GenerateEvent(event) => {
                    match event {
                        quotes::Event::QuoteReceived { peer, quote } => {
                            println!("GOT QUOTE FROM {peer}: {quote:?}");
                            self.cache.insert(peer, quote);
                            self.expiry.replace(
                                peer,
                                Box::pin(tokio::time::sleep(crate::defaults::CACHED_QUOTE_EXPIRY)),
                            );
                            self.emit_cached_quotes();
                        }
                        quotes::Event::DoesNotSupportProtocol { peer: _ } => {
                            // Don't care about this
                        }
                    }
                }
                _ => {
                    return Poll::Ready(event.map_out(|_| {
                        unreachable!("we handle all generate events in the arm above")
                    }));
                }
            }
        }

        if let Some(event) = self.to_swarm.pop_front() {
            return Poll::Ready(ToSwarm::GenerateEvent(event));
        }

        Poll::Pending
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

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        addr: &Multiaddr,
        role_override: libp2p::core::Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.inner
            .handle_established_outbound_connection(connection_id, peer, addr, role_override)
    }

    fn handle_pending_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<(), ConnectionDenied> {
        self.inner
            .handle_pending_inbound_connection(connection_id, local_addr, remote_addr)
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

        match event {
            FromSwarm::ConnectionEstablished(connection_established) => {
                // If we connected to a peer as a dialer, cache the address we connected to them at
                if let ConnectedPoint::Dialer { address, .. } = connection_established.endpoint {
                    self.addresses
                        .insert(connection_established.peer_id, address.clone());
                }
            }
            _ => {}
        }
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
}

impl From<Event> for out_event::bob::OutEvent {
    fn from(event: Event) -> Self {
        match event {
            Event::CachedQuotes { quotes } => Self::CachedQuotes { quotes },
        }
    }
}

impl From<Event> for out_event::alice::OutEvent {
    fn from(_: Event) -> Self {
        unreachable!("Alice should not use the cached quotes behaviour");
    }
}
