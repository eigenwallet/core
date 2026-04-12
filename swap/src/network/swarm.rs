use crate::asb::LatestRate;
use crate::network::rendezvous::XmrBtcNamespace;
use crate::seed::Seed;
use crate::{asb, cli};
use anyhow::Result;
use arti_client::TorClient;
use libp2p::connection_limits::ConnectionLimits;
use libp2p::swarm::NetworkBehaviour;
use libp2p::{Multiaddr, Swarm, identity};
use libp2p::{PeerId, SwarmBuilder};
use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;
use swap_core::bitcoin;
use swap_env::env;
use swap_p2p::libp2p_ext::MultiAddrExt;
use tor_hsservice::RunningOnionService;
use tor_rtcompat::tokio::TokioRustlsRuntime;

// We keep connections open for 2 minutes
const IDLE_CONNECTION_TIMEOUT: Duration = Duration::from_secs(60 * 2);

#[allow(clippy::too_many_arguments)]
pub fn asb<LR>(
    seed: &Seed,
    min_buy: bitcoin::Amount,
    max_buy: bitcoin::Amount,
    latest_rate: LR,
    resume_only: bool,
    env_config: env::Config,
    namespace: XmrBtcNamespace,
    rendezvous_addrs: &[Multiaddr],
    maybe_tor_client: Option<Arc<TorClient<TokioRustlsRuntime>>>,
    register_hidden_service: bool,
    num_intro_points: u8,
    max_concurrent_rend_requests: usize,
    wormhole_enabled: bool,
    wormhole_max_concurrent_rend_requests: usize,
    wormhole_num_intro_points: u8,
    wormhole_swap_freshness_hours: u64,
    trust_provider: Arc<dyn super::wormhole::PeerTrust + Send + Sync>,
) -> Result<(
    Swarm<asb::Behaviour<LR>>,
    Vec<Multiaddr>,
    Option<Arc<RunningOnionService>>,
)>
where
    LR: LatestRate + Send + 'static + Debug + Clone,
{
    let identity = seed.derive_libp2p_identity();

    let rendezvous_nodes: Vec<PeerId> = rendezvous_addrs
        .iter()
        .map(|addr| {
            addr.extract_peer_id()
                .expect("Rendezvous node address must contain peer ID")
        })
        .collect();

    // TODO: Prioritize honest peers in this queue
    let connection_limits = ConnectionLimits::default()
        // Limit peers stuck in the handshake phase
        .with_max_pending_incoming(Some(64 * 4))
        .with_max_established_incoming(Some(128 * 4))
        // A single peer only needs one connection; allow 4 for brief overlap during reconnects
        .with_max_established_per_peer(Some(4));

    let (transport, onion_addresses, wormhole_channels, onion_service_handle) =
        asb::transport::new(
            &identity,
            maybe_tor_client,
            register_hidden_service,
            num_intro_points,
            max_concurrent_rend_requests,
            wormhole_max_concurrent_rend_requests,
            wormhole_num_intro_points,
        )?;

    let behaviour = asb::Behaviour::new(
        min_buy,
        max_buy,
        latest_rate,
        resume_only,
        env_config,
        (identity.clone(), namespace),
        rendezvous_nodes,
        connection_limits,
        trust_provider,
        // Passing None disables the wormhole behaviour entirely.
        if wormhole_enabled {
            wormhole_channels
        } else {
            None
        },
        wormhole_swap_freshness_hours,
    );

    let mut swarm = SwarmBuilder::with_existing_identity(identity)
        .with_tokio()
        .with_other_transport(|_| transport)?
        .with_behaviour(|_| behaviour)?
        .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(IDLE_CONNECTION_TIMEOUT))
        .build();

    for addr in rendezvous_addrs {
        let peer_id = addr
            .extract_peer_id()
            .expect("Rendezvous node address must contain peer ID");
        swarm.add_peer_address(peer_id, addr.clone());
    }

    Ok((swarm, onion_addresses, onion_service_handle))
}

pub async fn cli<T>(
    identity: identity::Keypair,
    maybe_tor_client: Option<Arc<TorClient<TokioRustlsRuntime>>>,
    behaviour: T,
) -> Result<Swarm<T>>
where
    T: NetworkBehaviour,
{
    let transport = cli::transport::new(&identity, maybe_tor_client)?;

    let swarm = SwarmBuilder::with_existing_identity(identity)
        .with_tokio()
        .with_other_transport(|_| transport)?
        .with_behaviour(|_| behaviour)?
        .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(IDLE_CONNECTION_TIMEOUT))
        .build();

    Ok(swarm)
}
