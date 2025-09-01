use automerge::Change;
use libp2p::request_response::{ProtocolSupport};
use libp2p::swarm::NetworkBehaviour;
use libp2p::{ping, request_response, StreamProtocol};
use serde::{Deserialize, Serialize};
use std::ops::{Deref, DerefMut};
use std::time::Duration;
use tokio::sync::oneshot;

const PROTOCOL: &str = "/eigensync/1.0.0";

#[derive(NetworkBehaviour)]
pub struct Behaviour {
    ping: ping::Behaviour,
    sync: SyncBehaviour,
}

pub type SyncBehaviour = request_response::cbor::Behaviour<ServerRequest, Response>;

#[derive(Debug, Clone, Copy, Default)]
pub struct EigensyncProtocol;

impl AsRef<str> for EigensyncProtocol {
    fn as_ref(&self) -> &str {
        PROTOCOL
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct SerializedChange(Vec<u8>);

impl SerializedChange {
    pub fn new(data: Vec<u8>) -> Self {
        SerializedChange(data)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.0.clone()
    }
}

impl From<Change> for SerializedChange {
    fn from(mut change: Change) -> Self {
        SerializedChange(change.bytes().to_vec())
    }
}

impl From<SerializedChange> for Change {
    fn from(change: SerializedChange) -> Self {
        Change::from_bytes(change.0).unwrap()
    }
}

#[derive(Debug)]
pub struct ChannelRequest {
    pub changes: Vec<SerializedChange>,
    pub response_channel: oneshot::Sender<Result<Vec<SerializedChange>, String>>
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ServerRequest {
    UploadChangesToServer {
        changes: Vec<SerializedChange>
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Response {
    /// When the server has changes the device hasn't yet
    NewChanges {
        changes: Vec<SerializedChange>,
    },
    Error {
        reason: String,
    },
}

pub fn server() -> Behaviour {
    Behaviour {
        ping: ping::Behaviour::new(ping::Config::default()),
        sync: SyncBehaviour::new(
            vec![(
                StreamProtocol::new(EigensyncProtocol.as_ref()),
                ProtocolSupport::Inbound,
            )],
            request_response::Config::default()
                .with_request_timeout(Duration::from_secs(30))
                .with_max_concurrent_streams(10),
        ),
    }
}

pub fn client() -> Behaviour {
    Behaviour {
        ping: ping::Behaviour::new(ping::Config::default()),
        sync: SyncBehaviour::new(
            vec![(
                StreamProtocol::new(EigensyncProtocol.as_ref()),
                ProtocolSupport::Outbound,
            )],
            request_response::Config::default()
                .with_request_timeout(Duration::from_secs(30))
                .with_max_concurrent_streams(10),
        ),
    }
}

impl Deref for Behaviour {
    type Target = SyncBehaviour;

    fn deref(&self) -> &Self::Target {
        &self.sync
    }
}

impl DerefMut for Behaviour {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.sync
    }
}
