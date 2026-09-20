use std::num::NonZeroUsize;
use std::time::Duration;

use crate::common::tor::TorBackendSwap;
use crate::network::transport::authenticate_and_multiplex;
use anyhow::Result;
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::transport::Boxed;
use libp2p::{PeerId, Transport, identity};
use libp2p::{dns, tcp, websocket};
use libp2p_tor::{
    AddressConversion, TorDialLimiter, TorDialPriorityConfig, TorDialPriorityTracker, TorTransport,
};
use swap_tor::TorBackend;

// Higher priority gets more concurrency and tighter spacing; low priority gets
// the smallest budget.
const TOR_DIAL_HIGH_PRIORITY_MAX_CONCURRENT: usize = 5;
const TOR_DIAL_HIGH_PRIORITY_MIN_DELAY: Duration = Duration::from_millis(100);
const TOR_DIAL_NORMAL_PRIORITY_MAX_CONCURRENT: usize = 2;
const TOR_DIAL_NORMAL_PRIORITY_MIN_DELAY: Duration = Duration::from_secs(1);
const TOR_DIAL_LOW_PRIORITY_MAX_CONCURRENT: usize = 1;
const TOR_DIAL_LOW_PRIORITY_MIN_DELAY: Duration = Duration::from_secs(4);

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
    let low = TorDialPriorityConfig {
        max_concurrent: NonZeroUsize::new(TOR_DIAL_LOW_PRIORITY_MAX_CONCURRENT)
            .expect("TOR_DIAL_LOW_PRIORITY_MAX_CONCURRENT to be non-zero"),
        min_delay: TOR_DIAL_LOW_PRIORITY_MIN_DELAY,
    };

    let dial_limiter = TorDialLimiter::new(priority_tracker.clone(), high, normal, low);

    (dial_limiter, priority_tracker)
}

fn new_dns_transport(
    inner: tcp::tokio::Transport,
) -> std::io::Result<dns::tokio::Transport<tcp::tokio::Transport>> {
    if cfg!(target_os = "android") {
        return Ok(dns::tokio::Transport::custom(
            inner,
            dns::ResolverConfig::cloudflare(),
            dns::ResolverOpts::default(),
        ));
    }

    dns::tokio::Transport::system(inner)
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
    maybe_tor_client: TorBackend,
) -> Result<(
    Boxed<(PeerId, StreamMuxerBox)>,
    Option<TorDialPriorityTracker>,
)> {
    // Connection attempts through a SOCKS5 proxy are already limited by the
    // system Tor daemon, so only the internal Arti client gets a dial limiter.
    let (maybe_tor_dial_limiter, maybe_tor_priority_tracker) = match maybe_tor_client {
        TorBackend::Arti(..) => {
            let (dial_limiter, priority_tracker) = new_tor_dial_limiter();
            (Some(dial_limiter), Some(priority_tracker))
        }
        _ => (None, None),
    };

    // Build the websocket transport first. WsConfig strips the /ws suffix and
    // delegates to its inner transport, so we give it a Tor-or-TCP+DNS chain so
    // that ws connections are routed over Tor when available.
    let ws_inner = maybe_tor_client
        .clone()
        .into_transport(AddressConversion::IpAndDns, |transport| {
            match &maybe_tor_dial_limiter {
                Some(dial_limiter) => transport.with_dial_limiter(dial_limiter.clone()),
                None => transport,
            }
        })
        .map_err(anyhow::Error::from)?;
    let ws_transport = websocket::WsConfig::new(ws_inner);

    // Build the plain Tor-or-TCP+DNS transport for non-websocket addresses.
    let plain_transport = maybe_tor_client
        .into_transport(AddressConversion::IpAndDns, |transport| {
            match &maybe_tor_dial_limiter {
                Some(dial_limiter) => transport.with_dial_limiter(dial_limiter.clone()),
                None => transport,
            }
        })
        .map_err(anyhow::Error::from)?;

    // WsConfig only matches addresses ending in /ws or /wss, so it must come
    // first — otherwise Tor or TCP would eagerly claim the address (ignoring the
    // /ws suffix) and establish a raw connection without a WebSocket handshake.
    let transport = ws_transport.or_transport(plain_transport).boxed();

    Ok((
        authenticate_and_multiplex(transport, identity)?,
        maybe_tor_priority_tracker,
    ))
}
