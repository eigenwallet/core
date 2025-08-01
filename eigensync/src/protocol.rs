use libp2p::request_response::ProtocolSupport;
use libp2p::{request_response, PeerId, StreamProtocol};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

const PROTOCOL: &str = "/eigensync/1.0.0";
type OutEvent = request_response::Event<Request, Response>;
type Message = request_response::Message<Request, Response>;

pub type Behaviour = request_response::cbor::Behaviour<Request, Response>;

#[derive(Debug, Clone, Copy, Default)]
pub struct EigensyncProtocol;

impl AsRef<str> for EigensyncProtocol {
    fn as_ref(&self) -> &str {
        PROTOCOL
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub doc_id: Uuid,
    pub sync_msg: Vec<u8>, // Automerge sync message
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Response {
    SyncMsg { 
        doc_id: Uuid, 
        msg: Option<Vec<u8>> 
    },
    Error { 
        doc_id: Uuid, 
        reason: String 
    },
}

pub fn hub() -> Behaviour {
    Behaviour::new(
        vec![(
            StreamProtocol::new(EigensyncProtocol.as_ref()),
            ProtocolSupport::Full,
        )],
        request_response::Config::default()
            .with_request_timeout(Duration::from_secs(30))
            .with_max_concurrent_streams(10),
    )
}

pub fn device() -> Behaviour {
    Behaviour::new(
        vec![(
            StreamProtocol::new(EigensyncProtocol.as_ref()),
            ProtocolSupport::Full,
        )],
        request_response::Config::default()
            .with_request_timeout(Duration::from_secs(30))
            .with_max_concurrent_streams(10),
    )
}

/// Events that can be emitted by the sync protocol
#[derive(Debug)]
pub enum SyncEvent {
    IncomingSync {
        peer: PeerId,
        doc_id: Uuid,
        sync_msg: Vec<u8>,
        channel: request_response::ResponseChannel<Response>,
    },
    SyncResponse {
        doc_id: Uuid,
        msg: Option<Vec<u8>>,
    },
    SyncError {
        doc_id: Uuid,
        reason: String,
    },
    OutboundFailure {
        peer: PeerId,
        error: request_response::OutboundFailure,
    },
}

impl From<request_response::Event<Request, Response>> for SyncEvent {
    fn from(event: request_response::Event<Request, Response>) -> Self {
        match event {
            request_response::Event::Message { peer, message } => match message {
                request_response::Message::Request {
                    request, channel, ..
                } => Self::IncomingSync {
                    peer,
                    doc_id: request.doc_id,
                    sync_msg: request.sync_msg,
                    channel,
                },
                request_response::Message::Response { response, .. } => match response {
                    Response::SyncMsg { doc_id, msg } => Self::SyncResponse { doc_id, msg },
                    Response::Error { doc_id, reason } => Self::SyncError { doc_id, reason },
                },
            },
            request_response::Event::OutboundFailure { peer, error, .. } => {
                Self::OutboundFailure { peer, error }
            }
            request_response::Event::InboundFailure { .. } => {
                // Convert inbound failures to generic sync errors
                Self::SyncError {
                    doc_id: Uuid::nil(), // Placeholder since we don't have doc_id
                    reason: "Inbound connection failure".to_string(),
                }
            }
            request_response::Event::ResponseSent { .. } => {
                // Not particularly interesting for the application layer
                Self::SyncError {
                    doc_id: Uuid::nil(),
                    reason: "Response sent successfully".to_string(),
                }
            }
        }
    }
} 