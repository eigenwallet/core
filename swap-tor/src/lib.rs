use arti_client::TorClient;
use libp2p::core::multiaddr::Protocol;
use libp2p::core::transport::{ListenerId, TransportEvent};
use std::sync::Arc;
use tor_rtcompat::tokio::TokioRustlsRuntime;

pub type TcpTransport = libp2p::dns::tokio::Transport<libp2p::tcp::tokio::Transport>;

#[derive(Clone)]
pub enum TorBackend {
    /// Private Tor client
    Arti(Arc<TorClient<TokioRustlsRuntime>>),
    /// No Tor at all
    None,
}

impl std::fmt::Debug for TorBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        f.write_str(match self {
            TorBackend::Arti(..) => "Arti",
            TorBackend::None => "None",
        })
    }
}
