use libp2p::{identify, ping};
use libp2p::{
    request_response::{
        InboundFailure, InboundRequestId, OutboundFailure, OutboundRequestId, ResponseChannel,
    },
    Multiaddr, PeerId,
};

use crate::observe;
use crate::protocols::{
    cooperative_xmr_redeem_after_punish::CooperativeXmrRedeemRejectReason, quote::BidQuote,
    quotes_cached::QuoteStatus, transfer_proof,
};
use crate::protocols::{redial, rendezvous};

#[derive(Debug)]
pub enum OutEvent {
    QuoteReceived {
        id: OutboundRequestId,
        response: BidQuote,
    },
    CachedQuotes {
        quotes: Vec<(PeerId, Multiaddr, BidQuote, Option<semver::Version>)>,
    },
    CachedQuotesProgress {
        peers: Vec<(PeerId, QuoteStatus)>,
    },
    Observe(observe::Event),
    SwapSetupCompleted {
        peer: PeerId,
        swap_id: uuid::Uuid,
        result: Box<anyhow::Result<swap_machine::bob::State2>>,
    },
    TransferProofReceived {
        msg: Box<transfer_proof::Request>,
        channel: ResponseChannel<()>,
        peer: PeerId,
    },
    EncryptedSignatureAcknowledged {
        id: OutboundRequestId,
    },
    CooperativeXmrRedeemFulfilled {
        id: OutboundRequestId,
        swap_id: uuid::Uuid,
        s_a: swap_core::monero::Scalar,
        lock_transfer_proof: swap_core::monero::TransferProof,
    },
    CooperativeXmrRedeemRejected {
        id: OutboundRequestId,
        reason: CooperativeXmrRedeemRejectReason,
        swap_id: uuid::Uuid,
    },
    Failure {
        peer: PeerId,
        error: anyhow::Error,
    },
    OutboundRequestResponseFailure {
        peer: PeerId,
        error: OutboundFailure,
        request_id: OutboundRequestId,
        protocol: String,
    },
    InboundRequestResponseFailure {
        peer: PeerId,
        error: InboundFailure,
        request_id: InboundRequestId,
        protocol: String,
    },
    Redial(redial::Event),
    /// "Fallback" variant that allows the event mapping code to swallow certain
    /// events that we don't want the caller to deal with.
    Other,
}

impl OutEvent {
    pub fn unexpected_request(peer: PeerId) -> OutEvent {
        OutEvent::Failure {
            peer,
            error: anyhow::anyhow!("Unexpected request received"),
        }
    }

    pub fn unexpected_response(peer: PeerId) -> OutEvent {
        OutEvent::Failure {
            peer,
            error: anyhow::anyhow!("Unexpected response received"),
        }
    }
}

// Some other behaviours which are not worth their own module
impl From<ping::Event> for OutEvent {
    fn from(_: ping::Event) -> Self {
        OutEvent::Other
    }
}

impl From<identify::Event> for OutEvent {
    fn from(_: identify::Event) -> Self {
        OutEvent::Other
    }
}

impl From<rendezvous::discovery::Event> for OutEvent {
    fn from(_: rendezvous::discovery::Event) -> Self {
        OutEvent::Other
    }
}

impl From<observe::Event> for OutEvent {
    fn from(event: observe::Event) -> Self {
        OutEvent::Observe(event)
    }
}

impl From<()> for OutEvent {
    fn from(_: ()) -> Self {
        OutEvent::Other
    }
}
