use anyhow::Result;
use futures::future::BoxFuture;
use futures::{AsyncRead, AsyncWrite, Future, FutureExt};
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::transport::{Boxed, ListenerId, TransportError, TransportEvent};
use libp2p::core::upgrade::Version;
use libp2p::multiaddr::Protocol;
use libp2p::noise;
use libp2p::{Multiaddr, PeerId, Transport, identity, yamux};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use thiserror::Error;

const REGULAR_CONNECTION_SETUP_TIMEOUT: Duration = Duration::from_secs(15);
// Arti's onion-service proof-of-work can take significantly longer than a regular connection
// setup. This timeout is an upper bound around the entire onion dial; Arti still applies its own
// phase-specific timeouts while establishing the connection.
const ONION_CONNECTION_SETUP_TIMEOUT: Duration = Duration::from_secs(5 * 60);
// We have 5 protocols, not more than 2 of which should be active at the same time.
const MAX_NUM_STREAMS: usize = 5;

/// "Completes" a transport by applying the authentication and multiplexing
/// upgrades.
///
/// Even though the actual transport technology in use might be different, for
/// two libp2p applications to be compatible, the authentication and
/// multiplexing upgrades need to be compatible.
pub fn authenticate_and_multiplex<T>(
    transport: Boxed<T>,
    identity: &identity::Keypair,
) -> Result<Boxed<(PeerId, StreamMuxerBox)>>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let auth_upgrade = noise::Config::new(identity)?;
    let mut multiplex_upgrade = yamux::Config::default();

    multiplex_upgrade.set_max_num_streams(MAX_NUM_STREAMS);

    let transport = transport
        .upgrade(Version::V1)
        .authenticate(auth_upgrade)
        .multiplex(multiplex_upgrade);
    let transport = ConnectionSetupTimeout::new(
        transport,
        REGULAR_CONNECTION_SETUP_TIMEOUT,
        ONION_CONNECTION_SETUP_TIMEOUT,
    )
    .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
    .boxed();

    Ok(transport)
}

struct ConnectionSetupTimeout<T> {
    inner: T,
    regular_timeout: Duration,
    onion_timeout: Duration,
}

impl<T> ConnectionSetupTimeout<T> {
    fn new(inner: T, regular_timeout: Duration, onion_timeout: Duration) -> Self {
        Self {
            inner,
            regular_timeout,
            onion_timeout,
        }
    }

    fn outgoing_timeout(&self, addr: &Multiaddr) -> Duration {
        if addr
            .iter()
            .any(|protocol| matches!(protocol, Protocol::Onion3(_)))
        {
            return self.onion_timeout;
        }

        self.regular_timeout
    }
}

impl<T> Transport for ConnectionSetupTimeout<T>
where
    T: Transport + Unpin,
    T::Dial: Send + 'static,
    T::ListenerUpgrade: Send + 'static,
    T::Output: Send + 'static,
    T::Error: Send + Sync + 'static,
{
    type Output = T::Output;
    type Error = ConnectionSetupTimeoutError<T::Error>;
    type ListenerUpgrade = BoxFuture<'static, Result<Self::Output, Self::Error>>;
    type Dial = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn listen_on(
        &mut self,
        id: ListenerId,
        addr: Multiaddr,
    ) -> Result<(), TransportError<Self::Error>> {
        self.inner
            .listen_on(id, addr)
            .map_err(|error| error.map(ConnectionSetupTimeoutError::Transport))
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        self.inner.remove_listener(id)
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let timeout = self.outgoing_timeout(&addr);
        let dial = self
            .inner
            .dial(addr)
            .map_err(|error| error.map(ConnectionSetupTimeoutError::Transport))?;

        Ok(with_timeout(dial, timeout))
    }

    fn dial_as_listener(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        let timeout = self.outgoing_timeout(&addr);
        let dial = self
            .inner
            .dial_as_listener(addr)
            .map_err(|error| error.map(ConnectionSetupTimeoutError::Transport))?;

        Ok(with_timeout(dial, timeout))
    }

    fn address_translation(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.inner.address_translation(server, observed)
    }

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        let timeout = self.regular_timeout;

        Pin::new(&mut self.inner).poll(cx).map(|event| {
            event
                .map_upgrade(|upgrade| with_timeout(upgrade, timeout))
                .map_err(ConnectionSetupTimeoutError::Transport)
        })
    }
}

fn with_timeout<F, O, E>(
    future: F,
    timeout: Duration,
) -> BoxFuture<'static, Result<O, ConnectionSetupTimeoutError<E>>>
where
    F: Future<Output = Result<O, E>> + Send + 'static,
    O: Send + 'static,
    E: Send + 'static,
{
    async move {
        tokio::time::timeout(timeout, future)
            .await
            .map_err(|_| ConnectionSetupTimeoutError::Timeout(timeout))?
            .map_err(ConnectionSetupTimeoutError::Transport)
    }
    .boxed()
}

#[derive(Debug, Error)]
enum ConnectionSetupTimeoutError<E> {
    #[error("Connection setup timed out after {0:?}")]
    Timeout(Duration),
    #[error(transparent)]
    Transport(E),
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p::core::transport::dummy::DummyTransport;

    const ONION_ADDRESS: &str =
        "/onion3/lmnlknoxbmd2alyee5lihegdrjiyn6qzgbvapcz5klz3ptf7szi7dsqd:9939";

    #[test]
    fn onion_dials_use_the_longer_connection_setup_timeout() {
        let transport = ConnectionSetupTimeout::new(
            DummyTransport::<()>::new(),
            REGULAR_CONNECTION_SETUP_TIMEOUT,
            ONION_CONNECTION_SETUP_TIMEOUT,
        );

        assert_eq!(
            transport.outgoing_timeout(&ONION_ADDRESS.parse().unwrap()),
            ONION_CONNECTION_SETUP_TIMEOUT
        );
        assert_eq!(
            transport.outgoing_timeout(&"/ip4/127.0.0.1/tcp/9939".parse().unwrap()),
            REGULAR_CONNECTION_SETUP_TIMEOUT
        );
    }
}
