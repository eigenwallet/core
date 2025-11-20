use anyhow::{Context, Result};
use futures::{AsyncRead, AsyncWrite};
use libp2p::core::transport::Boxed;
use libp2p::core::upgrade::Version;
use libp2p::identity::{self};
use libp2p::tcp;
use libp2p::yamux;
use libp2p::{core::muxing::StreamMuxerBox, SwarmBuilder};
use libp2p::{dns, noise, Multiaddr, PeerId, Swarm, Transport};
use libp2p_tor::{AddressConversion, TorTransport};
use std::fmt;
use std::path::Path;
use tor_hsservice::config::OnionServiceConfigBuilder;

use crate::behaviour::Behaviour;
use crate::tor;

/// Defaults we use for the networking
mod defaults {
    use std::time::Duration;

    // We keep connections open for 10 minutes
    pub const IDLE_CONNECTION_TIMEOUT: Duration = Duration::from_secs(60 * 10);

    // Five intro points are a reasonable default
    pub const HIDDEN_SERVICE_NUM_INTRO_POINTS: u8 = 5;

    pub const MULTIPLEX_TIMEOUT: Duration = Duration::from_secs(60);
}

pub fn create_swarm(
    identity: identity::Keypair,
    rendezvous_addrs: Vec<Multiaddr>,
) -> Result<Swarm<Behaviour>> {
    let transport = create_transport(&identity).context("Failed to create transport")?;
    let behaviour = Behaviour::new(
        identity.clone(),
        rendezvous_addrs,
        swap_p2p::protocols::rendezvous::XmrBtcNamespace::RendezvousPoint,
    )?;

    let swarm = SwarmBuilder::with_existing_identity(identity)
        .with_tokio()
        .with_other_transport(|_| transport)?
        .with_behaviour(|_| behaviour)?
        .with_swarm_config(|cfg| {
            cfg.with_idle_connection_timeout(defaults::IDLE_CONNECTION_TIMEOUT)
        })
        .build();

    Ok(swarm)
}

pub async fn create_swarm_with_onion(
    identity: identity::Keypair,
    onion_port: u16,
    data_dir: &Path,
    rendezvous_addrs: Vec<Multiaddr>,
) -> Result<Swarm<Behaviour>> {
    let (transport, onion_address) = create_transport_with_onion(&identity, onion_port, data_dir)
        .await
        .context("Failed to create transport with onion")?;
    let behaviour = Behaviour::new(
        identity.clone(),
        rendezvous_addrs,
        swap_p2p::protocols::rendezvous::XmrBtcNamespace::RendezvousPoint,
    )?;

    let mut swarm = SwarmBuilder::with_existing_identity(identity)
        .with_tokio()
        .with_other_transport(|_| transport)?
        .with_behaviour(|_| behaviour)?
        .with_swarm_config(|cfg| {
            cfg.with_idle_connection_timeout(defaults::IDLE_CONNECTION_TIMEOUT)
        })
        .build();

    // Listen on the onion address
    swarm
        .listen_on(onion_address.clone())
        .context("Failed to listen on onion address")?;

    swarm.add_external_address(onion_address.clone());

    tracing::info!(%onion_address, "Onion service configured");

    Ok(swarm)
}

fn create_transport(identity: &identity::Keypair) -> Result<Boxed<(PeerId, StreamMuxerBox)>> {
    let tcp = tcp::tokio::Transport::new(tcp::Config::new().nodelay(true));
    let tcp_with_dns = dns::tokio::Transport::system(tcp)?;

    let transport = authenticate_and_multiplex(tcp_with_dns.boxed(), &identity).unwrap();

    Ok(transport)
}

async fn create_transport_with_onion(
    identity: &identity::Keypair,
    onion_port: u16,
    data_dir: &Path,
) -> Result<(Boxed<(PeerId, StreamMuxerBox)>, Multiaddr)> {
    // Create TCP transport
    let tcp = tcp::tokio::Transport::new(tcp::Config::new().nodelay(true));
    let tcp_with_dns = dns::tokio::Transport::system(tcp)?;

    // Create and bootstrap Tor client
    let tor_client = tor::create_tor_client(data_dir).await?;

    tokio::task::spawn(tor::bootstrap_tor_client(tor_client.clone()));

    // Create Tor transport from the bootstrapped client
    let mut tor_transport = TorTransport::from_client(tor_client, AddressConversion::IpAndDns);

    // Create onion service configuration
    let onion_service_config = OnionServiceConfigBuilder::default()
        .nickname(
            identity
                .public()
                .to_peer_id()
                .to_base58()
                .to_ascii_lowercase()
                .parse()
                .unwrap(),
        )
        .num_intro_points(defaults::HIDDEN_SERVICE_NUM_INTRO_POINTS)
        .build()
        .unwrap();

    // Add onion service and get the address
    let onion_address = tor_transport.add_onion_service(onion_service_config, onion_port)?;

    // Combine transports
    let combined_transport = tcp_with_dns
        .boxed()
        .or_transport(tor_transport.boxed())
        .boxed();

    let transport = authenticate_and_multiplex(combined_transport, &identity).unwrap();

    Ok((transport, onion_address))
}

fn authenticate_and_multiplex<T>(
    transport: Boxed<T>,
    identity: &identity::Keypair,
) -> Result<Boxed<(PeerId, StreamMuxerBox)>>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let noise_config = noise::Config::new(identity).unwrap();

    let transport = transport
        .upgrade(Version::V1)
        .authenticate(noise_config)
        .multiplex(yamux::Config::default())
        .timeout(defaults::MULTIPLEX_TIMEOUT)
        .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
        .boxed();

    Ok(transport)
}

pub struct Addresses<'a>(pub &'a [Multiaddr]);

// Prints an array of multiaddresses as a comma seperated string
impl fmt::Display for Addresses<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let display = self
            .0
            .iter()
            .map(|addr| addr.to_string())
            .collect::<Vec<String>>()
            .join(",");
        write!(f, "{}", display)
    }
}
