use libp2p::{
    PeerId,
    request_response::{
        InboundFailure, InboundRequestId, OutboundFailure, OutboundRequestId, ResponseChannel,
    },
};
use libp2p::{identify, ping};
use uuid::Uuid;

use crate::protocols::rendezvous;
use crate::protocols::{
    cooperative_xmr_redeem_after_punish, encrypted_signature, quote::BidQuote, swap_setup,
};

#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum OutEvent {
    SwapSetupInitiated {
        // run_swap_setup in connection handler sends us the amount of
        // Bitcoin Bob wants to send.
        // We respond with a snapshot of our wallets and how much of that
        // should go into the amnesty output
        send_wallet_snapshot: bmrng::RequestReceiver<
            bitcoin::Amount,
            (swap_setup::alice::WalletSnapshot, bitcoin::Amount, bool),
        >,
    },
    SwapSetupCompleted {
        peer_id: PeerId,
        swap_id: Uuid,
        state3: swap_machine::alice::State3,
    },
    SwapDeclined {
        peer: PeerId,
        error: swap_setup::alice::Error,
    },
    QuoteRequested {
        channel: ResponseChannel<BidQuote>,
        peer: PeerId,
    },
    TransferProofAcknowledged {
        peer: PeerId,
        id: OutboundRequestId,
    },
    EncryptedSignatureReceived {
        msg: encrypted_signature::Request,
        channel: ResponseChannel<()>,
        peer: PeerId,
    },
    CooperativeXmrRedeemRequested {
        channel: ResponseChannel<cooperative_xmr_redeem_after_punish::Response>,
        swap_id: Uuid,
        peer: PeerId,
    },
    Rendezvous(rendezvous::register::Event),
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
    Failure {
        peer: PeerId,
        error: anyhow::Error,
    },
    /// "Fallback" variant that allows the event mapping code to swallow
    /// certain events that we don't want the caller to deal with.
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

impl From<rendezvous::register::Event> for OutEvent {
    fn from(event: rendezvous::register::Event) -> Self {
        OutEvent::Rendezvous(event)
    }
}
