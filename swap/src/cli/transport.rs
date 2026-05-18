use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;

use crate::network::transport::authenticate_and_multiplex;
use anyhow::Result;
use arti_client::TorClient;
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::transport::{Boxed, OptionalTransport};
use libp2p::{PeerId, Transport, identity};
use libp2p::{dns, tcp, websocket};
use libp2p_tor::{
    AddressConversion, TorDialLimiter, TorDialPriorityConfig, TorDialPriorityTracker, TorTransport,
};
use tor_rtcompat::tokio::TokioRustlsRuntime;

// High-priority Tor dials get more concurrency and tighter spacing than
// normal ones.
const TOR_DIAL_HIGH_PRIORITY_MAX_CONCURRENT: usize = 2;
const TOR_DIAL_HIGH_PRIORITY_MIN_DELAY: Duration = Duration::from_millis(250);
const TOR_DIAL_NORMAL_PRIORITY_MAX_CONCURRENT: usize = 2;
const TOR_DIAL_NORMAL_PRIORITY_MIN_DELAY: Duration = Duration::from_secs(1);

fn new_tor_dial_limiter() -> (TorDialLimiter, TorDialPriorityTracker) {
    let priority_tracker = TorDialPriorityTracker::default();

    let high = TorDialPriorityConfig {
        max_concurrent: NonZeroUsize::new(TOR_DIAL_HIGH_PRIORITY_MAX_CONCURRENT)
            .expect("TOR_DIAL_HIGH_PRIORITY_MAX_CONCURRENT to be non-zero"),
        min_delay: TOR_DIAL_HIGH_PRIORITY_MIN_DELAY,
    };
    let normal = TorDialPriorityConfig {
        max_concurrent: NonZeroUsize::new(TOR_DIAL_NORMAL_PRIORITY_MAX_CONCURRENT)
            .expect("TOR_DIAL_NORMAL_PRIORITY_MAX_CONCURRENT to be non-zero"),
        min_delay: TOR_DIAL_NORMAL_PRIORITY_MIN_DELAY,
    };

    let dial_limiter = TorDialLimiter::new(priority_tracker.clone(), high, normal);

    (dial_limiter, priority_tracker)
}

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
) -> Result<(
    Boxed<(PeerId, StreamMuxerBox)>,
    Option<TorDialPriorityTracker>,
)> {
    let (maybe_tor_dial_limiter, maybe_tor_priority_tracker) = if maybe_tor_client.is_some() {
        let (dial_limiter, priority_tracker) = new_tor_dial_limiter();
        (Some(dial_limiter), Some(priority_tracker))
    } else {
        (None, None)
    };

    // Build the websocket transport first. WsConfig strips the /ws suffix and
    // delegates to its inner transport, so we give it a Tor-or-TCP+DNS chain so
    // that ws connections are routed over Tor when available.
    let ws_inner_tcp = tcp::tokio::Transport::new(tcp::Config::new().nodelay(true));
    let ws_inner_tcp_dns = dns::tokio::Transport::system(ws_inner_tcp)?;
    let ws_inner_tor: OptionalTransport<TorTransport> = match &maybe_tor_client {
        Some(client) => {
            let mut transport =
                TorTransport::from_client(Arc::clone(client), AddressConversion::IpAndDns);

            if let Some(dial_limiter) = maybe_tor_dial_limiter.clone() {
                transport = transport.with_dial_limiter(dial_limiter);
            }

            OptionalTransport::some(transport)
        }
        None => OptionalTransport::none(),
    };
    let ws_inner = ws_inner_tor.or_transport(ws_inner_tcp_dns);
    let ws_transport = websocket::WsConfig::new(ws_inner);

    // Build the plain Tor-or-TCP+DNS transport for non-websocket addresses.
    let tcp = tcp::tokio::Transport::new(tcp::Config::new().nodelay(true));
    let tcp_with_dns = dns::tokio::Transport::system(tcp)?;
    let maybe_tor_transport: OptionalTransport<TorTransport> = match maybe_tor_client {
        Some(client) => {
            let mut transport = TorTransport::from_client(client, AddressConversion::IpAndDns);

            if let Some(dial_limiter) = maybe_tor_dial_limiter {
                transport = transport.with_dial_limiter(dial_limiter);
            }

            OptionalTransport::some(transport)
        }
        None => OptionalTransport::none(),
    };
    let plain_transport = maybe_tor_transport.or_transport(tcp_with_dns);

    // WsConfig only matches addresses ending in /ws or /wss, so it must come
    // first — otherwise Tor or TCP would eagerly claim the address (ignoring the
    // /ws suffix) and establish a raw connection without a WebSocket handshake.
    let transport = ws_transport.or_transport(plain_transport).boxed();

    Ok((
        authenticate_and_multiplex(transport, identity)?,
        maybe_tor_priority_tracker,
    ))
}
