use crate::network::io_limit::{IoLimitedTransport, IoLimits};
use anyhow::Result;
use futures::{AsyncRead, AsyncWrite};
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::transport::Boxed;
use libp2p::core::upgrade::Version;
use libp2p::noise;
use libp2p::{PeerId, Transport, identity, yamux};
use std::time::Duration;

const AUTH_AND_MULTIPLEX_TIMEOUT: Duration = Duration::from_secs(15);
// We have 5 protocols, not more than 2 of which should be active at the same time.
const MAX_NUM_STREAMS: usize = 5;

/// "Completes" a transport by applying the authentication and multiplexing
/// upgrades.
///
/// Even though the actual transport technology in use might be different, for
/// two libp2p applications to be compatible, the authentication and
/// multiplexing upgrades need to be compatible.
///
/// `io_limits` bounds the raw connection throughput (globally across all
/// connections). Pass [`IoLimits::unlimited`] to disable rate limiting.
pub fn authenticate_and_multiplex<T>(
    transport: Boxed<T>,
    identity: &identity::Keypair,
    io_limits: &IoLimits,
) -> Result<Boxed<(PeerId, StreamMuxerBox)>>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let auth_upgrade = noise::Config::new(identity)?;
    let mut multiplex_upgrade = yamux::Config::default();

    multiplex_upgrade.set_max_num_streams(MAX_NUM_STREAMS);

    // Rate-limit the raw connection IO before it is authenticated and
    // multiplexed, so the limit applies to the actual bytes on the wire.
    let transport = IoLimitedTransport::new(transport, io_limits);

    let transport = transport
        .upgrade(Version::V1)
        .authenticate(auth_upgrade)
        .multiplex(multiplex_upgrade)
        .timeout(AUTH_AND_MULTIPLEX_TIMEOUT)
        .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
        .boxed();

    Ok(transport)
}
