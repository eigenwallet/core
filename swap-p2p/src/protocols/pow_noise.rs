//! This protocol wraps the noise authentication protocol.
//! It requires a proof of work challenge to be completed before
//! authentication is done.

use std::pin::Pin;

use futures::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, FutureExt};
use libp2p::{
    PeerId,
    core::{
        UpgradeInfo,
        upgrade::{InboundConnectionUpgrade, OutboundConnectionUpgrade},
    },
    noise::{self, Output},
};
use thiserror::Error;

#[derive(Clone)]
pub struct Config {
    inner: noise::Config,
    pow_difficulty: usize,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("noise error: {}", .0)]
    Noise(#[from] noise::Error),
    #[error("proof of work error: {}", .0)]
    Pow(#[from] PowError),
}

#[derive(Debug, Error)]
pub enum PowError {
    #[error("interface error: {}", .0)]
    Io(#[from] std::io::Error),
    #[error("peer sent invalid solution to pow challenge")]
    InvalidSolution,
}

impl Config {
    pub fn new(noise_config: noise::Config, pow_difficulty: usize) -> Self {
        Config {
            inner: noise_config,
            pow_difficulty,
        }
    }
}

impl UpgradeInfo for Config {
    type Info = &'static str;
    type InfoIter =
        std::iter::Chain<<noise::Config as UpgradeInfo>::InfoIter, std::iter::Once<Self::Info>>;

    fn protocol_info(&self) -> Self::InfoIter {
        self.inner.protocol_info().chain(std::iter::once("/pow"))
    }
}

impl<T> InboundConnectionUpgrade<T> for Config
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    type Output = (PeerId, noise::Output<T>);
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_inbound(self, mut socket: T, info: Self::Info) -> Self::Future {
        async move {
            let challenge = b"hello, world";
            socket.write_all(challenge).await.map_err(PowError::Io)?;
            let mut answer = [0u8; 12];
            socket.read_exact(&mut answer).await.map_err(PowError::Io)?;

            if &answer != b"this: answer" {
                tracing::warn!("Incoming connection failed proof of work challenge");
                return Err(PowError::InvalidSolution)?;
            }

            tracing::trace!("Incoming connection completed proof of work challenge");

            Ok(self.inner.upgrade_inbound(socket, info).await?)
        }
        .boxed()
    }
}

impl<T> OutboundConnectionUpgrade<T> for Config
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    type Output = (PeerId, Output<T>);
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_outbound(self, mut socket: T, info: Self::Info) -> Self::Future {
        async move {
            let mut challenge = [0u8; 12];
            socket
                .read_exact(&mut challenge)
                .await
                .map_err(PowError::Io)?;
            let answer = b"this: answer";
            socket.write_all(answer).await.map_err(PowError::Io)?;

            // TODO: find some way to check whether the peer accepted our solution.
            // One more packet from peer?

            Ok(self.inner.upgrade_outbound(socket, info).await?)
        }
        .boxed()
    }
}
