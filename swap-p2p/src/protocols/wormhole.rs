use libp2p::request_response::{self, ProtocolSupport};
use libp2p::{Multiaddr, StreamProtocol};
use serde::{Deserialize, Serialize};

const PROTOCOL: &str = "/comit/xmr/btc/wormhole/1.0.0";

pub type InnerBehaviour = request_response::cbor::Behaviour<Request, ()>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub address: Multiaddr,
    pub active: bool,
}

pub fn alice() -> InnerBehaviour {
    InnerBehaviour::new(
        vec![(StreamProtocol::new(PROTOCOL), ProtocolSupport::Outbound)],
        request_response::Config::default()
            .with_request_timeout(crate::defaults::DEFAULT_REQUEST_TIMEOUT),
    )
}

pub fn bob() -> InnerBehaviour {
    InnerBehaviour::new(
        vec![(StreamProtocol::new(PROTOCOL), ProtocolSupport::Inbound)],
        request_response::Config::default()
            .with_request_timeout(crate::defaults::DEFAULT_REQUEST_TIMEOUT),
    )
}
