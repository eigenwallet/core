use crate::out_event;
use libp2p::request_response::{self, ProtocolSupport};
use libp2p::{PeerId, StreamProtocol};
use serde::{Deserialize, Serialize};
use swap_core::bitcoin;
use typeshare::typeshare;

pub(crate) const PROTOCOL: &str = "/comit/xmr/btc/bid-quote/1.0.0";
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
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
#[typeshare]
pub struct BidQuote {
    /// The price at which the maker is willing to buy at.
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    #[typeshare(serialized_as = "number")]
    pub price: bitcoin::Amount,
    /// The minimum quantity the maker is willing to buy.
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    #[typeshare(serialized_as = "number")]
    pub min_quantity: bitcoin::Amount,
    /// The maximum quantity the maker is willing to buy.
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    #[typeshare(serialized_as = "number")]
    pub max_quantity: bitcoin::Amount,
    /// Monero "ReserveProofV2" which proves that Alice has the funds to fulfill the quote.
    /// See "Zero to Monero" section 8.1.6 for more details.
    ///
    /// The message used when signing the proof is the peer ID of the peer that generated the quote.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reserve_proof: Option<ReserveProofWithAddress>,
}

impl BidQuote {
    /// A zero quote with all amounts set to zero and with no reserve proof
    pub const ZERO: Self = Self {
        price: bitcoin::Amount::ZERO,
        min_quantity: bitcoin::Amount::ZERO,
        max_quantity: bitcoin::Amount::ZERO,
        reserve_proof: None,
    };
}

#[derive(Clone, Copy, Debug, thiserror::Error)]
#[error("Received quote of 0")]
pub struct ZeroQuoteReceived;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
#[typeshare]
pub struct ReserveProofWithAddress {
    #[serde(with = "swap_serde::monero::address_serde")]
    #[typeshare(serialized_as = "string")]
    pub address: monero_address::MoneroAddress,
    pub proof: String,
    // TOOD: Technically redundant as convention tells us its the peer id but it'd be nice to be able to verify reserve proofs isolatedly
    pub message: String,
}

/// Constructs a new instance of the `quote` behaviour to be used by the ASB.
///
/// The ASB is always listening and only supports inbound connections, i.e.
/// handing out quotes.
pub fn alice() -> Behaviour {
    Behaviour::new(
        vec![(StreamProtocol::new(PROTOCOL), ProtocolSupport::Inbound)],
        request_response::Config::default()
            .with_request_timeout(crate::defaults::QUOTE_REQUEST_TIMEOUT),
    )
}

/// Constructs a new instance of the `quote` behaviour to be used by the CLI.
///
/// The CLI is always dialing and only supports outbound connections, i.e.
/// requesting quotes.
pub fn bob() -> Behaviour {
    Behaviour::new(
        vec![(StreamProtocol::new(PROTOCOL), ProtocolSupport::Outbound)],
        request_response::Config::default()
            .with_request_timeout(crate::defaults::QUOTE_REQUEST_TIMEOUT),
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
