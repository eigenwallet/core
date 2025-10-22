use std::sync::Arc;

use crate::common::tor::tor_client_to_transport;
use crate::network::transport::authenticate_and_multiplex;
use anyhow::Result;
use arti_client::TorClient;
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::transport::Boxed;
use libp2p::{identity, PeerId, Transport};
use libp2p_tor::AddressConversion;
use tor_rtcompat::tokio::TokioRustlsRuntime;

/// Creates the libp2p transport for the swap CLI.
///
/// The CLI's transport needs the following capabilities:
/// - Establish TCP connections
/// - Resolve DNS entries
/// - Dial onion-addresses through a running Tor daemon by connecting to the
///   socks5 port. If the port is not given, we will fall back to the regular
///   TCP transport.
pub fn new(
    identity: &identity::Keypair,
    maybe_tor_client: Option<Arc<TorClient<TokioRustlsRuntime>>>,
) -> Result<Boxed<(PeerId, StreamMuxerBox)>> {
    let transport = tor_client_to_transport(maybe_tor_client, AddressConversion::IpAndDns, |_| {})?;
    authenticate_and_multiplex(transport.boxed(), identity)
}
