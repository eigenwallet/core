use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use crate::network::transport::authenticate_and_multiplex;
use anyhow::Result;
use arti_client::TorClient;
use data_encoding::BASE32_NOPAD;
use futures::FutureExt;
use futures::future::BoxFuture;
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::transport::{
    Boxed, ListenerId, OptionalTransport, TransportError, TransportEvent,
};
use libp2p::multiaddr::Protocol;
use libp2p::{Multiaddr, PeerId, Transport, identity};
use libp2p::{dns, tcp, websocket};
use libp2p_tor::{AddressConversion, TorTransport};
use tokio_util::compat::TokioAsyncReadCompatExt;
use tor_rtcompat::tokio::TokioRustlsRuntime;

/// Create the libp2p transport for the swap CLI.
pub fn new(
    identity: &identity::Keypair,
    maybe_tor_client: Option<Arc<TorClient<TokioRustlsRuntime>>>,
) -> Result<Boxed<(PeerId, StreamMuxerBox)>> {
    if maybe_tor_client.is_none() && tor_socks5::is_enabled() {
        return new_with_system_socks5(identity);
    }

    if maybe_tor_client.is_none() {
        return new_clearnet(identity);
    }

    // `WsConfig` strips the `/ws` suffix and delegates to its inner transport.
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

    // Plain transport for non-websocket addresses.
    let tcp = tcp::tokio::Transport::new(tcp::Config::new().nodelay(true));
    let tcp_with_dns = dns::tokio::Transport::system(tcp)?;
    let maybe_tor_transport: OptionalTransport<TorTransport> = match maybe_tor_client {
        Some(client) => OptionalTransport::some(TorTransport::from_client(
            client,
            AddressConversion::IpAndDns,
        )),
        None => OptionalTransport::none(),
    };
    let plain_transport = maybe_tor_transport.or_transport(tcp_with_dns);

    // Put `WsConfig` first so `/ws` and `/wss` get the WebSocket handshake.
    let transport = ws_transport.or_transport(plain_transport).boxed();

    authenticate_and_multiplex(transport, identity)
}

/// Clearnet-only transport (no Tor, no SOCKS5).
fn new_clearnet(identity: &identity::Keypair) -> Result<Boxed<(PeerId, StreamMuxerBox)>> {
    let ws_inner_tcp = tcp::tokio::Transport::new(tcp::Config::new().nodelay(true));
    let ws_inner = dns::tokio::Transport::system(ws_inner_tcp)?;
    let ws_transport = websocket::WsConfig::new(ws_inner);

    let tcp = tcp::tokio::Transport::new(tcp::Config::new().nodelay(true));
    let plain_transport = dns::tokio::Transport::system(tcp)?;

    let transport = ws_transport.or_transport(plain_transport).boxed();
    authenticate_and_multiplex(transport, identity)
}

/// SOCKS5 transport with direct TCP fallback for local addresses.
fn new_with_system_socks5(identity: &identity::Keypair) -> Result<Boxed<(PeerId, StreamMuxerBox)>> {
    let proxy = tor_socks5::proxy_config(tor_socks5::Subsystem::Libp2p)
        .expect("libp2p SOCKS5 proxy config must exist when system Tor is enabled");

    let local_tcp_plain = tcp::tokio::Transport::new(tcp::Config::new().nodelay(true));
    let local_tcp_ws = tcp::tokio::Transport::new(tcp::Config::new().nodelay(true));

    let ws_inner = Socks5Transport::new(proxy).or_transport(local_tcp_ws);
    let ws_transport = websocket::WsConfig::new(ws_inner);
    let plain_transport = Socks5Transport::new(proxy).or_transport(local_tcp_plain);

    let transport = ws_transport.or_transport(plain_transport).boxed();
    authenticate_and_multiplex(transport, identity)
}

type Socks5Stream =
    tokio_util::compat::Compat<tokio_socks::tcp::Socks5Stream<tokio::net::TcpStream>>;

struct Socks5Transport {
    proxy: tor_socks5::ProxyConfig,
}

impl Socks5Transport {
    fn new(proxy: tor_socks5::ProxyConfig) -> Self {
        Self { proxy }
    }

    fn extract_target(addr: &Multiaddr) -> Option<(String, u16)> {
        let mut iter = addr.iter();

        let host = match iter.next()? {
            Protocol::Onion3(onion) => {
                let encoded = BASE32_NOPAD.encode(onion.hash()).to_lowercase();
                return Some((format!("{encoded}.onion"), onion.port()));
            }
            Protocol::Dns4(host) | Protocol::Dns6(host) => host.into_owned(),
            Protocol::Ip4(ip) => ip.to_string(),
            Protocol::Ip6(ip) => ip.to_string(),
            _ => return None,
        };

        if tor_socks5::is_local_host(&host) {
            return None;
        }

        let Protocol::Tcp(port) = iter.next()? else {
            return None;
        };
        Some((host, port))
    }
}

impl Transport for Socks5Transport {
    type Output = Socks5Stream;
    type Error = io::Error;
    type Dial = BoxFuture<'static, Result<Self::Output, Self::Error>>;
    type ListenerUpgrade = futures::future::Pending<Result<Self::Output, Self::Error>>;

    fn listen_on(
        &mut self,
        _id: ListenerId,
        addr: Multiaddr,
    ) -> Result<(), TransportError<Self::Error>> {
        Err(TransportError::MultiaddrNotSupported(addr))
    }

    fn remove_listener(&mut self, _id: ListenerId) -> bool {
        false
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let (host, port) = Self::extract_target(&addr)
            .ok_or_else(|| TransportError::MultiaddrNotSupported(addr))?;

        let proxy = self.proxy;

        Ok(async move {
            let stream = proxy
                .connect((host, port))
                .await
                .map_err(|e| io::Error::other(e.to_string()))?;

            Ok(stream.compat())
        }
        .boxed())
    }

    fn dial_as_listener(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        self.dial(addr)
    }

    fn address_translation(&self, _listen: &Multiaddr, _observed: &Multiaddr) -> Option<Multiaddr> {
        None
    }

    fn poll(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn target_of(s: &str) -> Option<(String, u16)> {
        let addr = Multiaddr::from_str(s).expect("valid test multiaddr");
        Socks5Transport::extract_target(&addr)
    }

    #[test]
    fn extract_target_resolves_ip_and_dns_hosts() {
        assert_eq!(
            target_of("/ip4/1.2.3.4/tcp/9050"),
            Some(("1.2.3.4".to_string(), 9050))
        );
        assert_eq!(
            target_of("/ip6/2001:db8::1/tcp/80"),
            Some(("2001:db8::1".to_string(), 80))
        );
        assert_eq!(
            target_of("/dns4/example.com/tcp/443"),
            Some(("example.com".to_string(), 443))
        );
        assert_eq!(
            target_of("/dns6/example.com/tcp/443"),
            Some(("example.com".to_string(), 443))
        );
    }

    #[test]
    fn extract_target_skips_local_hosts() {
        // Local addresses use the direct TCP fallback.
        assert_eq!(target_of("/ip4/127.0.0.1/tcp/9050"), None);
        assert_eq!(target_of("/ip4/192.168.1.1/tcp/80"), None);
        assert_eq!(target_of("/ip6/::1/tcp/80"), None);
        assert_eq!(target_of("/dns4/localhost/tcp/9050"), None);
    }

    #[test]
    fn extract_target_rejects_non_tcp_second_protocol() {
        // Only TCP targets are supported.
        assert_eq!(target_of("/ip4/1.2.3.4/udp/9050"), None);
    }

    #[test]
    fn extract_target_rejects_unsupported_leading_protocol() {
        // Leading `/p2p/...` has no dial target for SOCKS5.
        assert_eq!(
            target_of("/p2p/12D3KooWGQmdpzHXCqLno4mMxWXKNFQHASBeF99gTm2JR8Vu5Bdc"),
            None
        );
    }

    #[test]
    fn extract_target_encodes_onion3() {
        // Use a fixed onion hash to keep the expected host stable.
        let hash = [0u8; 35];
        let onion = libp2p::multiaddr::Onion3Addr::from((hash, 1234));
        let addr = Multiaddr::empty().with(Protocol::Onion3(onion));

        let (host, port) = Socks5Transport::extract_target(&addr).expect("onion target");
        assert_eq!(port, 1234);
        assert!(host.ends_with(".onion"), "host must end in .onion, got {host}");
        assert_eq!(host, host.to_lowercase(), "onion host must be lowercase");
        let stripped = host.strip_suffix(".onion").unwrap();
        assert_eq!(stripped.len(), 56, "base32 of 35 bytes must be 56 chars");
    }
}
