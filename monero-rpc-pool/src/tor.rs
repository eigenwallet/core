use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use swap_tor::TorBackend;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_socks::TargetAddr;

/// Trait alias for a stream that can be used with hyper
pub trait HyperStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> HyperStream for T {}

#[allow(async_fn_in_trait)]
pub trait TorBackendRpc {
    fn is_some(&self) -> bool;
    fn ready_for_traffic(&self) -> bool;
    fn masquerade_clearnet(&self) -> bool;
    async fn connect(&self, address: (&str, u16)) -> anyhow::Result<Box<dyn HyperStream>>;
}
impl TorBackendRpc for TorBackend {
    fn is_some(&self) -> bool {
        !matches!(self, TorBackend::None)
    }

    fn ready_for_traffic(&self) -> bool {
        match self {
            TorBackend::Arti(arti) => arti.bootstrap_status().ready_for_traffic(),
            TorBackend::Socks(..) => true,
            TorBackend::None => false,
        }
    }

    fn masquerade_clearnet(&self) -> bool {
        match self {
            TorBackend::Arti(..) | TorBackend::None => false,
            TorBackend::Socks(..) => true,
        }
    }

    async fn connect(&self, address: (&str, u16)) -> anyhow::Result<Box<dyn HyperStream>> {
        match self {
            TorBackend::Arti(tor_client) => Ok(Box::new(tor_client.connect(address).await?)),
            TorBackend::Socks(proxy) => Ok(Box::new(proxy.proxy(pair_to_socks(address)).await?)),
            TorBackend::None => Ok(Box::new(tokio::net::TcpStream::connect(address).await?)),
        }
    }
}

// Parse order matches tokio::net::ToSocketAddrs
fn pair_to_socks((host, port): (&'_ str, u16)) -> TargetAddr<'_> {
    if let Ok(addr) = host.parse::<Ipv4Addr>() {
        TargetAddr::Ip(SocketAddr::new(addr.into(), 10))
    } else if let Ok(addr) = host.parse::<Ipv6Addr>() {
        TargetAddr::Ip(SocketAddr::new(addr.into(), 10))
    } else {
        TargetAddr::Domain(host.into(), port)
    }
}
#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
    use tokio_socks::TargetAddr;

    #[test]
    fn pair_to_socks() {
        assert_eq!(
            [
                ("ip.tld", 10),
                ("dns.ip4.tld", 11),
                ("dns.ip6.tld", 12),
                (
                    "cebulka7uxchnbpvmqapg5pfos4ngaxglsktzvha7a5rigndghvadeyd.onion",
                    13
                ),
                ("127.0.0.1", 10),
                ("::1", 10),
            ]
            .map(super::pair_to_socks),
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
