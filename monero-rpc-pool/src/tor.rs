use swap_tor::TorBackend;
use tokio::io::{AsyncRead, AsyncWrite};

/// Trait alias for a stream that can be used with hyper
pub trait HyperStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> HyperStream for T {}

#[allow(async_fn_in_trait)]
pub trait TorBackendRpc {
    fn is_some(&self) -> bool;
    fn ready_for_traffic(&self) -> bool;
    async fn connect(&self, address: (&str, u16)) -> anyhow::Result<Box<dyn HyperStream>>;
}
impl TorBackendRpc for TorBackend {
    fn is_some(&self) -> bool {
        !matches!(self, TorBackend::None)
    }

    fn ready_for_traffic(&self) -> bool {
        match self {
            TorBackend::Arti(arti) => arti.bootstrap_status().ready_for_traffic(),
            TorBackend::None => false,
        }
    }

    async fn connect(&self, address: (&str, u16)) -> anyhow::Result<Box<dyn HyperStream>> {
        match self {
            TorBackend::Arti(tor_client) => Ok(Box::new(tor_client.connect(address).await?)),
            TorBackend::None => Ok(Box::new(tokio::net::TcpStream::connect(address).await?)),
        }
    }
}
