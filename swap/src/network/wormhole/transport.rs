use std::pin::Pin;
use std::task::{Context, Poll};

use futures::future::BoxFuture;
use libp2p::core::transport::{ListenerId, TransportEvent};
use libp2p::{Multiaddr, Transport, TransportError};
use libp2p_tor::{TokioTorStream, TorTransport, TorTransportError};
use tokio::sync::mpsc;
use tor_hsservice::config::OnionServiceConfigBuilder;

use super::ServiceRequest;

/// Port used for wormhole onion services.
const WORMHOLE_SERVICE_PORT: u16 = 9939;
/// Max concurrent rendezvous requests for wormhole services.
/// Lower than the main service since these serve a single peer.
const WORMHOLE_MAX_CONCURRENT_REND_REQUESTS: usize = 2;
const WORMHOLE_NUM_INTRO_POINTS: u8 = 3;

/// A wrapper around `TorTransport` that can dynamically spawn dedicated
/// onion services at runtime by receiving requests through a channel.
pub struct WormholeTransport {
    inner: TorTransport,
    service_rx: mpsc::UnboundedReceiver<ServiceRequest>,
}

impl WormholeTransport {
    pub fn new(inner: TorTransport, service_rx: mpsc::UnboundedReceiver<ServiceRequest>) -> Self {
        Self { inner, service_rx }
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
            let svc_cfg = match OnionServiceConfigBuilder::default()
                .nickname(
                    request
                        .nickname
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

            let addr = match self.inner.add_onion_service_with_hsid(
                svc_cfg,
                request.keypair,
                WORMHOLE_SERVICE_PORT,
                WORMHOLE_MAX_CONCURRENT_REND_REQUESTS,
            ) {
                Ok(addr) => addr,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to add wormhole onion service");
                    continue;
                }
            };

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
