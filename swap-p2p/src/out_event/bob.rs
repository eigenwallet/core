use libp2p::{PeerId, request_response::{InboundFailure, InboundRequestId, OutboundFailure, OutboundRequestId, ResponseChannel}};
use libp2p::{identify, ping};

use crate::protocols::{cooperative_xmr_redeem_after_punish::CooperativeXmrRedeemRejectReason, quote::BidQuote, transfer_proof};

#[derive(Debug)]
pub enum OutEvent {
    QuoteReceived {
        id: OutboundRequestId,
        response: BidQuote,
    },
    SwapSetupCompleted(Box<anyhow::Result<swap_machine::bob::State2>>),
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

impl From<()> for OutEvent {
    fn from(_: ()) -> Self {
        OutEvent::Other
    }
}
