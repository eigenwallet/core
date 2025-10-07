use libp2p::request_response::ProtocolSupport;
use libp2p::swarm::NetworkBehaviour;
use libp2p::{request_response, StreamProtocol};
use std::time::Duration;

use super::*;

const PROTOCOL: &str = "/eigenwallet/pinning/pin/1.0.0";

pub type Behaviour = request_response::cbor::Behaviour<Request, Response>;
pub type ToSwarm = <Behaviour as NetworkBehaviour>::ToSwarm;

#[derive(Serialize, Deserialize)]
pub enum Request {
    Pin(PinRequest),
    Pull(PullRequest),
}

#[derive(Serialize, Deserialize)]
pub enum Response {
    Pin(PinResponse),
    Pull(PullResponse),
}

pub fn client(timeout: Duration) -> Behaviour {
    Behaviour::new(
        vec![(StreamProtocol::new(PROTOCOL), ProtocolSupport::Outbound)],
        request_response::Config::default().with_request_timeout(timeout),
    )
}

pub fn server(timeout: Duration) -> Behaviour {
    Behaviour::new(
        vec![(StreamProtocol::new(PROTOCOL), ProtocolSupport::Inbound)],
        request_response::Config::default().with_request_timeout(timeout),
    )
}
