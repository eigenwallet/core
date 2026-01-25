use crate::out_event;
use libp2p::request_response::{self, ProtocolSupport};
use libp2p::{PeerId, StreamProtocol};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use swap_core::bitcoin;
use swap_env::config::RefundPolicy;
use typeshare::typeshare;

pub(crate) const PROTOCOL: &str = "/comit/xmr/btc/bid-quote/2.0.0";
pub type OutEvent = request_response::Event<(), BidQuote>;
pub type Message = request_response::Message<(), BidQuote>;

pub type Behaviour = request_response::json::Behaviour<(), BidQuote>;

/// The refund policy that will apply if the swap is cancelled.
/// Communicated in quotes so takers know the terms upfront.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
#[serde(tag = "type", content = "content")]
#[typeshare]
pub enum RefundPolicyWire {
    /// Taker receives 100% of their Bitcoin back on refund.
    FullRefund,
    /// Taker receives a partial refund; the remainder goes to an amnesty output
    /// that the maker may or may not release later.
    PartialRefund {
        /// Ratio (0.0-1.0) of Bitcoin the taker receives immediately.
        #[typeshare(serialized_as = "number")]
        taker_refund_ratio: Decimal,
    },
}

impl From<RefundPolicy> for RefundPolicyWire {
    fn from(policy: RefundPolicy) -> Self {
        if policy.taker_refund_ratio == Decimal::ONE {
            RefundPolicyWire::FullRefund
        } else {
            RefundPolicyWire::PartialRefund {
                taker_refund_ratio: policy.taker_refund_ratio,
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BidQuoteProtocol;

impl AsRef<str> for BidQuoteProtocol {
    fn as_ref(&self) -> &str {
        PROTOCOL
    }
}

/// Represents a quote for buying XMR.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[typeshare]
pub struct BidQuote {
    /// The price at which the maker is willing to buy at.
    #[typeshare(serialized_as = "number")]
    pub price: bitcoin::Amount,
    /// The minimum quantity the maker is willing to buy.
    #[typeshare(serialized_as = "number")]
    pub min_quantity: bitcoin::Amount,
    /// The maximum quantity the maker is willing to buy.
    #[typeshare(serialized_as = "number")]
    pub max_quantity: bitcoin::Amount,
    /// The refund policy that will apply if the swap is cancelled.
    pub refund_policy: RefundPolicyWire,
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
        refund_policy: RefundPolicyWire::FullRefund,
        reserve_proof: None,
    };
}

#[derive(Clone, Copy, Debug, thiserror::Error)]
#[error("Received quote of 0")]
pub struct ZeroQuoteReceived;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
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
