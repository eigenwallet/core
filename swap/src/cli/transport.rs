use std::sync::Arc;

use crate::network::transport::authenticate_and_multiplex;
use anyhow::Result;
use arti_client::TorClient;
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::transport::{Boxed, OptionalTransport};
use libp2p::{dns, tcp, websocket};
use libp2p::{PeerId, Transport, identity};
use libp2p_tor::{AddressConversion, TorTransport};
use tor_rtcompat::tokio::TokioRustlsRuntime;

/// Creates the libp2p transport for the swap CLI.
///
/// The CLI's transport needs the following capabilities:
/// - Establish TCP connections
/// - Resolve DNS entries
/// - Dial websocket addresses (ws), including over Tor
/// - Dial onion-addresses through a running Tor daemon by connecting to the
///   socks5 port. If the port is not given, we will fall back to the regular
///   TCP transport.
pub fn new(
    identity: &identity::Keypair,
    maybe_tor_client: Option<Arc<TorClient<TokioRustlsRuntime>>>,
) -> Result<Boxed<(PeerId, StreamMuxerBox)>> {
    // Build the websocket transport first. WsConfig strips the /ws suffix and
    // delegates to its inner transport, so we give it a Tor-or-TCP+DNS chain so
    // that ws connections are routed over Tor when available.
    let ws_inner_tcp = tcp::tokio::Transport::new(tcp::Config::new().nodelay(true));
    let ws_inner_tcp_dns = dns::tokio::Transport::system(ws_inner_tcp)?;
    let ws_inner_tor: OptionalTransport<TorTransport> = match &maybe_tor_client {
        Some(client) => OptionalTransport::some(TorTransport::from_client(
            Arc::clone(client),
            AddressConversion::IpAndDns,
        )),
        None => OptionalTransport::none(),
    };
    let ws_inner = ws_inner_tor.or_transport(ws_inner_tcp_dns);
    let ws_transport = websocket::WsConfig::new(ws_inner);

    // Build the plain Tor-or-TCP+DNS transport for non-websocket addresses.
    let tcp = tcp::tokio::Transport::new(tcp::Config::new().nodelay(true));
    let tcp_with_dns = dns::tokio::Transport::system(tcp)?;
    let maybe_tor_transport: OptionalTransport<TorTransport> = match maybe_tor_client {
        Some(client) => {
            OptionalTransport::some(TorTransport::from_client(client, AddressConversion::IpAndDns))
        }
        None => OptionalTransport::none(),
    };
    let plain_transport = maybe_tor_transport.or_transport(tcp_with_dns);

    // WsConfig only matches addresses ending in /ws or /wss, so it must come
    // first — otherwise Tor or TCP would eagerly claim the address (ignoring the
    // /ws suffix) and establish a raw connection without a WebSocket handshake.
    let transport = ws_transport.or_transport(plain_transport).boxed();

    authenticate_and_multiplex(transport, identity)
}
