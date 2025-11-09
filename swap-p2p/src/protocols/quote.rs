use std::time::Duration;

use crate::out_event;
use libp2p::request_response::{self, ProtocolSupport};
use libp2p::{PeerId, StreamProtocol};
use serde::{Deserialize, Serialize};
use swap_core::bitcoin;
use typeshare::typeshare;

const PROTOCOL: &str = "/comit/xmr/btc/bid-quote/1.0.0";
pub type OutEvent = request_response::Event<(), BidQuote>;
pub type Message = request_response::Message<(), BidQuote>;

pub type Behaviour = request_response::json::Behaviour<(), BidQuote>;

#[derive(Debug, Clone, Copy, Default)]
pub struct BidQuoteProtocol;

impl AsRef<str> for BidQuoteProtocol {
    fn as_ref(&self) -> &str {
        PROTOCOL
    }
}

/// Represents a quote for buying XMR.
#[derive(Serialize, Deserialize, Debug, Copy, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[typeshare]
pub struct BidQuote {
    /// The price at which the maker is willing to buy at.
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    #[typeshare(serialized_as = "number")]
    pub price: bitcoin::Amount,
    /// The minimum quantity the maker is willing to buy.
    ///     #[typeshare(serialized_as = "number")]
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    #[typeshare(serialized_as = "number")]
    pub min_quantity: bitcoin::Amount,
    /// The maximum quantity the maker is willing to buy.
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    #[typeshare(serialized_as = "number")]
    pub max_quantity: bitcoin::Amount,
}

impl BidQuote {
    /// A zero quote with all amounts set to zero
    pub const ZERO: Self = Self {
        price: bitcoin::Amount::ZERO,
        min_quantity: bitcoin::Amount::ZERO,
        max_quantity: bitcoin::Amount::ZERO,
    };
}

#[derive(Clone, Copy, Debug, thiserror::Error)]
#[error("Received quote of 0")]
pub struct ZeroQuoteReceived;

/// Constructs a new instance of the `quote` behaviour to be used by the ASB.
///
/// The ASB is always listening and only supports inbound connections, i.e.
/// handing out quotes.
pub fn asb() -> Behaviour {
    Behaviour::new(
        vec![(StreamProtocol::new(PROTOCOL), ProtocolSupport::Inbound)],
        request_response::Config::default().with_request_timeout(Duration::from_secs(60)),
    )
}

/// Constructs a new instance of the `quote` behaviour to be used by the CLI.
///
/// The CLI is always dialing and only supports outbound connections, i.e.
/// requesting quotes.
pub fn cli() -> Behaviour {
    Behaviour::new(
        vec![(StreamProtocol::new(PROTOCOL), ProtocolSupport::Outbound)],
        request_response::Config::default().with_request_timeout(Duration::from_secs(60)),
    )
}

impl From<(PeerId, Message)> for out_event::alice::OutEvent {
    fn from((peer, message): (PeerId, Message)) -> Self {
        match message {
            Message::Request { channel, .. } => Self::QuoteRequested { channel, peer },
            Message::Response { .. } => Self::unexpected_response(peer),
        }
    }
}
crate::impl_from_rr_event!(OutEvent, out_event::alice::OutEvent, PROTOCOL);

impl From<(PeerId, Message)> for out_event::bob::OutEvent {
    fn from((peer, message): (PeerId, Message)) -> Self {
        match message {
            Message::Request { .. } => Self::unexpected_request(peer),
            Message::Response {
                response,
                request_id,
            } => Self::QuoteReceived {
                id: request_id,
                response,
            },
        }
    }
}
crate::impl_from_rr_event!(OutEvent, out_event::bob::OutEvent, PROTOCOL);

/// Behaviour that listens for peers that support the protocol and then periodically requests a quote from them.
pub mod background {
    use futures::future;
    use libp2p::{Multiaddr, PeerId, StreamProtocol, core::Endpoint, swarm::{ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, THandlerInEvent, THandlerOutEvent, ToSwarm}};

    use std::task::Poll;

    use crate::{futures_util::FuturesHashSet, protocols::{notice, quote}}; // TODO: use super import here

    pub struct Behaviour {
        inner: InnerBehaviour,
        to_request: FuturesHashSet<PeerId, ()>,
        to_request_tx_rx: (tokio::sync::mpsc::Sender<PeerId>, tokio::sync::mpsc::Receiver<PeerId>),
    }

    impl Behaviour {
        pub fn new() -> Self {
            // TODO: Make unbounded
            let (to_request_tx, to_request_rx) = tokio::sync::mpsc::channel(100);

            Self {
                inner: InnerBehaviour {
                    quote: quote::cli(),
                    notice: notice::Behaviour::new(StreamProtocol::new(quote::PROTOCOL)),
                },
                to_request: FuturesHashSet::new(),
                to_request_tx_rx: (to_request_tx, to_request_rx),
            }
        }

        pub async fn send_request(&mut self, peer: &PeerId) {
            let _ = self.to_request_tx_rx.0.send(*peer).await;
        }
    }

    #[derive(NetworkBehaviour)]
    pub struct InnerBehaviour {
        quote: quote::Behaviour,
        notice: notice::Behaviour,
    }

    impl libp2p::swarm::NetworkBehaviour for Behaviour {
        type ConnectionHandler = <InnerBehaviour as libp2p::swarm::NetworkBehaviour>::ConnectionHandler;
        type ToSwarm = <InnerBehaviour as libp2p::swarm::NetworkBehaviour>::ToSwarm;

        fn handle_pending_outbound_connection(
            &mut self,
            connection_id: libp2p::swarm::ConnectionId,
            maybe_peer: Option<PeerId>,
            addresses: &[Multiaddr],
            effective_role: libp2p::core::Endpoint,
        ) -> Result<Vec<Multiaddr>, libp2p::swarm::ConnectionDenied> {
            // Forward to inner behaviour so sub-behaviours (like request-response)
            // can contribute addresses to dial attempts.
            self.inner.handle_pending_outbound_connection(
                connection_id,
                maybe_peer,
                addresses,
                effective_role,
            )
        }

        fn handle_established_inbound_connection(
            &mut self,
            connection_id: ConnectionId,
            peer: PeerId,
            local_addr: &Multiaddr,
            remote_addr: &Multiaddr,
        ) -> Result<libp2p::swarm::THandler<Self>, ConnectionDenied> {
            self.inner.handle_established_inbound_connection(connection_id, peer, local_addr, remote_addr)
        }

        fn handle_established_outbound_connection(
            &mut self,
            connection_id: ConnectionId,
            peer: PeerId,
            addr: &Multiaddr,
            role_override: Endpoint,
        ) -> Result<libp2p::swarm::THandler<Self>, ConnectionDenied> {
            self.inner.handle_established_outbound_connection(connection_id, peer, addr, role_override)
        }

        fn on_swarm_event(&mut self, event: FromSwarm<'_>) {
            self.inner.on_swarm_event(event)
        }

        fn on_connection_handler_event(
            &mut self,
            peer_id: PeerId,
            connection_id: ConnectionId,
            event: THandlerOutEvent<Self>,
        ) {
            self.inner.on_connection_handler_event(peer_id, connection_id, event)
        }

        fn poll(
            &mut self,
            cx: &mut std::task::Context<'_>,
        ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
            while let Poll::Ready(Some(peer)) = self.to_request_tx_rx.1.poll_recv(cx) {
                tracing::trace!(%peer, "Queueing quote request to peer which we got from outside the behaviour");
                self.to_request.insert(peer, Box::pin(future::ready(())));
            }

            while let Poll::Ready(Some((peer, ()))) = self.to_request.poll_next_unpin(cx) {
                let outbound_request_id = self.inner.quote.send_request(&peer, ());
                tracing::trace!(%
                    peer,
                    %outbound_request_id,
                    "Dispatching outgoing quote request to peer");
            }

            let inner_poll = self.inner.poll(cx);

            if let Poll::Ready(ToSwarm::GenerateEvent(event)) = inner_poll {
                match event {
                    InnerBehaviourEvent::Notice(notice::Event::SupportsProtocol { peer }) => {
                        tracing::trace!(%peer, "Queuing quote request to peer after noticing that they support the quote protocol");
                        self.to_request.insert(peer, Box::pin(future::ready(())));
                    },
                    other => {
                        return Poll::Ready(ToSwarm::GenerateEvent(other));
                    }
                }
            }

            Poll::Pending
        }
    }
}
