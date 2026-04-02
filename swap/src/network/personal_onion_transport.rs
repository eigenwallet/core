use std::pin::Pin;
use std::task::{Context, Poll};

use futures::future::BoxFuture;
use libp2p::core::transport::{ListenerId, TransportEvent};
use libp2p::{Multiaddr, Transport, TransportError};
use libp2p_tor::{TokioTorStream, TorTransport, TorTransportError};
use swap_p2p::protocols::personal_onion::PersonalServiceRequest;
use tokio::sync::mpsc;
use tor_hsservice::config::OnionServiceConfigBuilder;

/// Port used for personal onion services.
const PERSONAL_SERVICE_PORT: u16 = 9939;
/// Max concurrent rendezvous requests for personal services.
/// Lower than the main service since these serve a single peer.
const PERSONAL_MAX_CONCURRENT_REND_REQUESTS: usize = 4;

/// A wrapper around `TorTransport` that can dynamically spawn personal
/// hidden services at runtime by receiving requests through a channel.
pub struct PersonalOnionTransport {
    inner: TorTransport,
    service_rx: mpsc::UnboundedReceiver<PersonalServiceRequest>,
}

impl PersonalOnionTransport {
    pub fn new(
        inner: TorTransport,
        service_rx: mpsc::UnboundedReceiver<PersonalServiceRequest>,
    ) -> Self {
        Self { inner, service_rx }
    }
}

impl Transport for PersonalOnionTransport {
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
        // Drain the channel for new personal service requests
        while let Poll::Ready(Some(request)) = self.service_rx.poll_recv(cx) {
            let svc_cfg = match OnionServiceConfigBuilder::default()
                .nickname(
                    request
                        .nickname
                        .parse()
                        .expect("Personal service nickname to be valid"),
                )
                .num_intro_points(3)
                .enable_pow(false)
                .build()
            {
                Ok(cfg) => cfg,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to build personal onion service config");
                    continue;
                }
            };

            let addr = match self.inner.add_onion_service_with_hsid(
                svc_cfg,
                request.keypair,
                PERSONAL_SERVICE_PORT,
                PERSONAL_MAX_CONCURRENT_REND_REQUESTS,
            ) {
                Ok(addr) => addr,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to add personal onion service");
                    continue;
                }
            };

            let listener_id = ListenerId::next();
            if let Err(e) = self.inner.listen_on(listener_id, addr.clone()) {
                tracing::error!(%addr, error = %e, "Failed to listen on personal onion service");
            }
        }

        // Delegate to inner transport
        Pin::new(&mut self.inner).poll(cx)
    }
}
