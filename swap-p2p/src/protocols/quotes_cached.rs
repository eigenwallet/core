use crate::behaviour_util::{AddressTracker};
use crate::futures_util::FuturesHashSet;
use crate::out_event;
use crate::protocols::quote::BidQuote;
use crate::protocols::quotes;
use libp2p::identify;
use libp2p::swarm::{
    ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, THandler, THandlerInEvent,
    THandlerOutEvent, ToSwarm,
};
use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::task::{Context, Poll};
use typeshare::typeshare;

pub struct Behaviour {
    inner: quotes::Behaviour,

    /// For each peer, cache the address we last connected to them at
    // TODO: Technically this is not required. The UI gets the address from the observe behaviour.
    address_tracker: AddressTracker,

    /// For every peer track the last semver version we received from them
    // TODO: Maybe let these expire after a certain time?
    versions: HashMap<PeerId, semver::Version>,

    // Caches quotes
    // TODO: Maybe move the identify logic from quotes to quotes_cached?
    cache: HashMap<PeerId, BidQuote>,
    quote_status: HashMap<PeerId, QuoteStatus>,
    expiry: FuturesHashSet<PeerId, ()>,

    // Queue of events to be sent to the swarm
    to_swarm: VecDeque<Event>,
}

impl Behaviour {
    pub fn new(identify_config: identify::Config) -> Self {
        Self {
            inner: quotes::Behaviour::new(identify_config),
            address_tracker: AddressTracker::new(),
            versions: HashMap::new(),
            cache: HashMap::new(),
            quote_status: HashMap::new(),
            expiry: FuturesHashSet::new(),
            to_swarm: VecDeque::new(),
        }
    }

    fn emit_cached_quotes(&mut self) {
        // Attach the address we last connected to the peer at to the quote
        //
        // Ignores those peers where we don't have an address cached
        let quotes: Vec<(PeerId, Multiaddr, BidQuote, Option<semver::Version>)> = self
            .cache
            .iter()
            .filter_map(|(peer_id, quote)| {
                self.address_tracker.last_seen_address(peer_id).map(|addr| {
                    let version = self.versions.get(peer_id).cloned();

                    (*peer_id, addr, quote.clone(), version)
                })
            })
            .collect();

        // There is no way to receive a quote from a peer without ever hearing about any address from them
        debug_assert_eq!(quotes.len(), self.cache.len());

        self.to_swarm.push_back(Event::CachedQuotes { quotes });
    }

    fn emit_progress(&mut self) {
        let mut peers = Vec::new();

        let all_peers: HashSet<PeerId> = self
            .address_tracker
            .peers()
            .chain(self.quote_status.keys())
            .cloned()
            .collect();

        for peer in all_peers {
            let quote = self
                .quote_status
                .get(&peer)
                .cloned()
                .unwrap_or(QuoteStatus::Nothing);

            peers.push((peer, quote));
        }

        self.to_swarm.push_back(Event::Progress { peers });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[typeshare]
pub enum QuoteStatus {
    Received,
    NotSupported,
    Inflight,
    Failed,
    Nothing,
}

#[derive(Debug)]
pub enum Event {
    CachedQuotes {
        // Peer ID, Address, Quote, Version
        quotes: Vec<(PeerId, Multiaddr, BidQuote, Option<semver::Version>)>,
    },
    Progress {
        peers: Vec<(PeerId, QuoteStatus)>,
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
                            self.cache.insert(peer, quote);
                            self.quote_status.insert(peer, QuoteStatus::Received);
                            self.expiry.replace(
                                peer,
                                Box::pin(tokio::time::sleep(crate::defaults::CACHED_QUOTE_EXPIRY)),
                            );

                            self.emit_cached_quotes();
                            self.emit_progress();
                        }
                        quotes::Event::QuoteInflight { peer } => {
                            self.quote_status.insert(peer, QuoteStatus::Inflight);
                            self.emit_progress();
                        }
                        quotes::Event::QuoteFailed { peer } => {
                            self.quote_status.insert(peer, QuoteStatus::Failed);
                            self.emit_progress();
                        }
                        quotes::Event::VersionReceived { peer, version } => {
                            self.versions.insert(peer, version);

                            // TODO: Only emit if the version is different from the cached one?
                            self.emit_cached_quotes();
                            self.emit_progress();
                        }
                        quotes::Event::DoesNotSupportProtocol { peer } => {
                            self.quote_status.insert(peer, QuoteStatus::NotSupported);
                            self.emit_progress();
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
        self.address_tracker.handle_swarm_event(event);
        self.emit_progress(); // TODO: this will emit quite frequently
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
}

impl From<Event> for out_event::bob::OutEvent {
    fn from(event: Event) -> Self {
        match event {
            Event::CachedQuotes { quotes } => Self::CachedQuotes { quotes },
            Event::Progress { peers } => Self::CachedQuotesProgress { peers },
        }
    }
}

impl From<Event> for out_event::alice::OutEvent {
    fn from(_: Event) -> Self {
        unreachable!("Alice should not use the cached quotes behaviour");
    }
}
