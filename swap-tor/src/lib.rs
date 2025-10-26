use arti_client::TorClient;
use libp2p::core::multiaddr::Protocol;
use libp2p::core::transport::{ListenerId, TransportEvent};
use libp2p::{Multiaddr, Transport, TransportError};
use std::fs;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::net::TcpStream;
use tokio_socks::tcp::Socks5Stream;
use tokio_socks::TargetAddr;
use tor_rtcompat::tokio::TokioRustlsRuntime;

fn onion3_to_dotonion(service: &[u8; 35]) -> String {
    let mut domain = data_encoding::BASE32.encode(service).to_lowercase();
    domain.push_str(".onion");
    domain
}
fn multi_to_socks(addr: &Multiaddr) -> Option<TargetAddr<'static>> {
    let mut addr = addr.iter();
    match (addr.next()?, addr.next()) {
        (
            Protocol::Dns(domain) | Protocol::Dns4(domain) | Protocol::Dns6(domain),
            Some(Protocol::Tcp(port)),
        ) => Some(TargetAddr::Domain(domain.into_owned().into(), port)),
        (Protocol::Onion3(service), _) => Some(TargetAddr::Domain(
            onion3_to_dotonion(service.hash()).into(),
            service.port(),
        )),
        (Protocol::Ip4(ip), Some(Protocol::Tcp(port))) => {
            Some(TargetAddr::Ip(SocketAddr::from((ip, port))))
        }
        (Protocol::Ip6(ip), Some(Protocol::Tcp(port))) => {
            Some(TargetAddr::Ip(SocketAddr::from((ip, port))))
        }
        _ => None,
    }
}
#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
    use tokio_socks::TargetAddr;

    const MULTIS: [&str; 6] = [
        "/dns/ip.tld/tcp/10",
        "/dns4/dns.ip4.tld/tcp/11",
        "/dns6/dns.ip6.tld/tcp/12",
        "/onion3/cebulka7uxchnbpvmqapg5pfos4ngaxglsktzvha7a5rigndghvadeyd:13",
        "/ip4/127.0.0.1/tcp/10",
        "/ip6/::1/tcp/10",
    ];

    #[test]
    fn multi_to_socks() {
        assert_eq!(
            MULTIS.map(|ma| super::multi_to_socks(&ma.parse().unwrap()).unwrap()),
            [
                TargetAddr::Domain("ip.tld".into(), 10),
                TargetAddr::Domain("dns.ip4.tld".into(), 11),
                TargetAddr::Domain("dns.ip6.tld".into(), 12),
                TargetAddr::Domain(
                    "cebulka7uxchnbpvmqapg5pfos4ngaxglsktzvha7a5rigndghvadeyd.onion".into(),
                    13,
                ),
                TargetAddr::Ip(SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 10)),
                TargetAddr::Ip(SocketAddr::new(Ipv6Addr::LOCALHOST.into(), 10)),
            ],
        );
    }
}

pub struct Socks5Transport(SocksServerAddress);
impl Transport for Socks5Transport {
    type Output = tokio_util::compat::Compat<TcpStream>;
    type Error = tokio_socks::Error;
    type ListenerUpgrade = std::future::Pending<Result<Self::Output, Self::Error>>;
    type Dial = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send + 'static>>;

    fn listen_on(
        &mut self,
        _: ListenerId,
        addr: Multiaddr,
    ) -> Result<(), TransportError<Self::Error>> {
        Err(TransportError::MultiaddrNotSupported(addr))
    }

    fn remove_listener(&mut self, _: ListenerId) -> bool {
        false
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let target = multi_to_socks(&addr).ok_or(TransportError::MultiaddrNotSupported(addr))?;
        let proxy = self.0;

        Ok(Box::pin(async move {
            Ok(tokio_util::compat::TokioAsyncReadCompatExt::compat(
                proxy.proxy(target).await?,
            ))
        }))
    }

    fn dial_as_listener(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        self.dial(addr)
    }

    fn poll(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        Poll::Pending
    }

    fn address_translation(&self, _: &Multiaddr, _: &Multiaddr) -> Option<Multiaddr> {
        None
    }
}

#[derive(Debug, Copy, Clone)]
pub struct SocksServerAddress(pub SocketAddr);

impl SocksServerAddress {
    pub fn transport(self) -> Socks5Transport {
        tracing::debug!("Using SOCKS5 proxy at {:?}", self.0);
        Socks5Transport(self)
    }

    pub async fn connect(&self) -> std::io::Result<TcpStream> {
        TcpStream::connect(self.0).await
    }

    pub async fn proxy(&self, target: TargetAddr<'_>) -> Result<TcpStream, tokio_socks::Error> {
        Socks5Stream::connect_with_socket(self.connect().await?, target)
            .await
            .map(Socks5Stream::into_inner)
    }
}

pub type TcpTransport = libp2p::dns::tokio::Transport<libp2p::tcp::tokio::Transport>;

#[derive(Clone)]
pub enum TorBackend {
    /// Private Tor client
    Arti(Arc<TorClient<TokioRustlsRuntime>>),
    /// Talking through a Tor SOCKS5 proxy
    Socks(SocksServerAddress),
    /// No Tor at all
    None,
}

impl std::fmt::Debug for TorBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        f.write_str(match self {
            TorBackend::Arti(..) => "Arti",
            TorBackend::Socks(..) => "Socks",
            TorBackend::None => "None",
        })
    }
}

#[derive(Copy, Clone, Debug)]
pub enum SpecialTorEnvironment {
    /// Torsocksed userland, Tor control and SOCKS5 in `$TOR_...`, well-known SOCKS5 at `127.0.0.1:9050`
    ///
    /// The `$TOR_...` configuration uses unix-domain sockets which we'd have to wrap ourselves
    ///
    /// We can't support a hypothetical `TorsocksTransport` that'd substitute
    ///   /onion3/cebulka7uxchnbpvmqapg5pfos4ngaxglsktzvha7a5rigndghvadeyd:13
    /// with
    ///   /dns/cebulka7uxchnbpvmqapg5pfos4ngaxglsktzvha7a5rigndghvadeyd.onion/tcp/13
    /// then forward to TcpTransport,
    /// because [hickory-resolver refuses to resolve `.onion` addresses](https://github.com/hickory-dns/hickory-dns/issues/3331).
    Whonix,
    /// Well-known SOCKS5 at `127.0.0.1:9050`, cf. `/usr/local/bin/curl`
    ///
    /// Userland pretends it's torsocksed but dialling actually doesn't work at all; *all* network traffic must go through SOCKS5.
    Tails,
}

pub static TOR_ENVIRONMENT: once_cell::sync::Lazy<Option<SpecialTorEnvironment>> =
    once_cell::sync::Lazy::new(|| {
        if fs::exists("/usr/share/whonix/marker").unwrap_or(false) {
            Some(SpecialTorEnvironment::Whonix)
        } else if fs::read_to_string("/etc/os-release")
            .unwrap_or(String::new())
            .contains(r#"ID="tails""#)
        {
            Some(SpecialTorEnvironment::Tails)
        } else {
            None
        }
    });

impl SpecialTorEnvironment {
    pub fn backend(self) -> TorBackend {
        match self {
            Self::Whonix | Self::Tails => TorBackend::Socks(SocksServerAddress(
                (std::net::Ipv4Addr::LOCALHOST, 9050).into(),
            )),
        }
    }

    /// `true` if listening on an address like `/ip4/0.0.0.0/tcp/9939` is possible in this environment
    pub fn can_listen_tcp(self) -> bool {
        match self {
            Self::Whonix | Self::Tails => false,
        }
    }

    /// `true` if listening on an address like `/onion3/whatever` is possible in this environment
    pub fn can_listen_onion(self) -> bool {
        match self {
            Self::Whonix | Self::Tails => false,
        }
    }

    /// Explain to the user why Tor is always on
    pub fn excuse(self) -> String {
        format!("Under {self:?}, the app always uses the global Tor connection.")
    }
}
