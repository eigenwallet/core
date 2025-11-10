use libp2p::request_response::ProtocolSupport;
use libp2p::swarm::NetworkBehaviour;
use libp2p::{request_response, StreamProtocol};
use std::time::Duration;

use super::*;

const PROTOCOL: &str = "/eigenwallet/pinning/pin/1.0.0";

pub type Behaviour = request_response::cbor::Behaviour<Request, Response>;
pub type Event = <Behaviour as NetworkBehaviour>::ToSwarm;

#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    Pin(pin::Request),
    Pull(pull::Request),
    Fetch(fetch::Request),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    // TODO: Maybe don't use std::Result here and use our own type?
    // TOOD: I'd like to own all of our types in our protocol codec (as many as possible)
    Pin(Result<pin::Response, pin::Error>),
    Pull(Result<pull::Response, pull::Error>),
    Fetch(Result<fetch::Response, fetch::Error>),
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
