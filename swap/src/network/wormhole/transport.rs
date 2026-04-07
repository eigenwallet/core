use std::pin::Pin;
use std::task::{Context, Poll};

use futures::future::BoxFuture;
use libp2p::core::transport::{ListenerId, TransportEvent};
use libp2p::{Multiaddr, Transport, TransportError};
use libp2p_tor::{TokioTorStream, TorTransport, TorTransportError};
use tokio::sync::mpsc;
use tor_hsservice::config::OnionServiceConfigBuilder;

use super::{ServiceHandle, ServiceRequest};

/// Port used for wormhole onion services.
const WORMHOLE_SERVICE_PORT: u16 = 9939;
const WORMHOLE_NUM_INTRO_POINTS: u8 = 3;

/// Channel handles returned by [`WormholeTransport::new`] for the behaviour
/// to send requests and receive service handles.
pub struct WormholeChannels {
    /// Send spawn requests to the transport.
    pub service_tx: mpsc::UnboundedSender<ServiceRequest>,
    /// Receive running service handles back from the transport.
    pub handle_rx: mpsc::UnboundedReceiver<ServiceHandle>,
}

/// A wrapper around `TorTransport` that can dynamically spawn dedicated
/// onion services at runtime by receiving requests through a channel.
pub struct WormholeTransport {
    inner: TorTransport,
    service_rx: mpsc::UnboundedReceiver<ServiceRequest>,
    handle_tx: mpsc::UnboundedSender<ServiceHandle>,
    max_concurrent_rend_requests: usize,
}

impl WormholeTransport {
    pub fn new(
        inner: TorTransport,
        max_concurrent_rend_requests: usize,
    ) -> (Self, WormholeChannels) {
        let (service_tx, service_rx) = mpsc::unbounded_channel();
        let (handle_tx, handle_rx) = mpsc::unbounded_channel();

        let transport = Self {
            inner,
            service_rx,
            handle_tx,
            max_concurrent_rend_requests,
        };

        let channels = WormholeChannels {
            service_tx,
            handle_rx,
        };

        (transport, channels)
    }
}

impl Transport for WormholeTransport {
    type Output = TokioTorStream;
    type Error = TorTransportError;
    type Dial = BoxFuture<'static, Result<Self::Output, Self::Error>>;
    type ListenerUpgrade = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn listen_on(
        &mut self,
        id: ListenerId,
        addr: Multiaddr,
    ) -> Result<(), TransportError<Self::Error>> {
        self.inner.listen_on(id, addr)
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        self.inner.remove_listener(id)
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        self.inner.dial(addr)
    }

    fn dial_as_listener(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        self.inner.dial_as_listener(addr)
    }

    fn address_translation(&self, listen: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.inner.address_translation(listen, observed)
    }

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        // Drain the channel for new wormhole service requests
        while let Poll::Ready(Some(request)) = self.service_rx.poll_recv(cx) {
            // Arti nicknames: 1-20 chars, [a-zA-Z0-9_] only.
            // Hash the peer_id for uniform distribution, take first 8 hex chars.
            use bitcoin::hashes::{Hash, sha256};
            let hash = sha256::Hash::hash(&request.peer_id.to_bytes());
            let hex = data_encoding::HEXLOWER.encode(&hash.to_byte_array()[..8]);
            let nickname = format!("wh_{hex}");
            let svc_cfg = match OnionServiceConfigBuilder::default()
                .nickname(
                    nickname
                        .parse()
                        .expect("Wormhole service nickname to be valid"),
                )
                .num_intro_points(WORMHOLE_NUM_INTRO_POINTS)
                .enable_pow(true)
                .build()
            {
                Ok(cfg) => cfg,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to build wormhole onion service config");
                    continue;
                }
            };

            let max_rend = self.max_concurrent_rend_requests;
            let (addr, service) = match self.inner.add_onion_service_with_hsid(
                svc_cfg,
                request.keypair,
                WORMHOLE_SERVICE_PORT,
                max_rend,
            ) {
                Ok(result) => result,
                Err(e) => {
                    tracing::error!(error = ?e, "Failed to add wormhole onion service");
                    continue;
                }
            };

            let _ = self.handle_tx.send(ServiceHandle {
                peer_id: request.peer_id,
                service,
            });

            let listener_id = ListenerId::next();
            if let Err(e) = self.inner.listen_on(listener_id, addr.clone()) {
                tracing::error!(%addr, error = %e, "Failed to listen on wormhole onion service");
            } else {
                tracing::info!(%addr, "Wormhole onion service started");
            }
        }

        // Delegate to inner transport
        Pin::new(&mut self.inner).poll(cx)
    }
}
