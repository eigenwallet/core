pub use crate::network::rendezvous;
use crate::network::rendezvous::XmrBtcNamespace;
use crate::network::swap_setup::alice;
use crate::network::transport::authenticate_and_multiplex;
use crate::network::{
    cooperative_xmr_redeem_after_punish, encrypted_signature, quote, transfer_proof,
};
use anyhow::Result;
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::transport::Boxed;
use libp2p::swarm::NetworkBehaviour;
use libp2p::{Multiaddr, PeerId};
use std::time::Duration;
use swap_env::env;
use swap_feed::LatestRate;

pub mod transport {
    use std::sync::Arc;

    use arti_client::{TorClient, config::onion_service::OnionServiceConfigBuilder};
    use libp2p::{Transport, core::transport::OptionalTransport, dns, identity, tcp, websocket};
    use libp2p_tor::AddressConversion;
    use tor_rtcompat::tokio::TokioRustlsRuntime;

    use crate::network::wormhole::alice::transport::{WormholeChannels, WormholeTransport};
    use tor_hsservice::RunningOnionService;

    use super::*;

    static ASB_ONION_SERVICE_NICKNAME: &str = "asb";
    static ASB_ONION_SERVICE_PORT: u16 = 9939;

    /// (transport, onion listen addresses, wormhole channels, primary onion service handle)
    type TransportResult = (
        Boxed<(PeerId, StreamMuxerBox)>,
        Vec<Multiaddr>,
        Option<WormholeChannels>,
        Option<Arc<RunningOnionService>>,
    );

    /// Creates the libp2p transport for the ASB.
    ///
    /// If you pass in a `None` for `maybe_tor_client`, the ASB will not use Tor at all.
    ///
    /// If you pass in a `Some(tor_client)`, the ASB will listen on an onion service and return
    /// the onion address. If it fails to listen on the onion address, it will only use tor for
    /// dialing and not listening.
    pub fn new(
        identity: &identity::Keypair,
        maybe_tor_client: Option<Arc<TorClient<TokioRustlsRuntime>>>,
        register_hidden_service: bool,
        num_intro_points: u8,
        max_concurrent_rend_requests: usize,
        wormhole_max_concurrent_rend_requests: usize,
        wormhole_num_intro_points: u8,
    ) -> Result<TransportResult> {
        // Streams are multiplexed via yamux, we don't really need more than one.
        const MAX_STREAMS_PER_CIRCUIT: u32 = 4;
        // This does not affect the PoW directly (only very slightly) but only serves as a protection
        // against memory exhaustion attacks when the queue of intro request fills up
        // We therefore set it to a fairly high value because there is barely any harm in doing so.
        // `MAX_CONCURRENT_REND_REQUESTS` is much more important in terms of DOS protection.
        const POW_QUEUE_DEPTH: usize = 2048;

        let (maybe_tor_transport, onion_addresses, wormhole_channels, onion_service_handle) =
            if let Some(tor_client) = maybe_tor_client {
                let mut tor_transport =
                    libp2p_tor::TorTransport::from_client(tor_client, AddressConversion::DnsOnly);

                let (addresses, onion_handle) = if register_hidden_service {
                    let onion_service_config = OnionServiceConfigBuilder::default()
                        .nickname(
                            ASB_ONION_SERVICE_NICKNAME
                                .parse()
                                .expect("Static nickname to be valid"),
                        )
                        .num_intro_points(num_intro_points)
                        // DOS mitigations
                        .max_concurrent_streams_per_circuit(MAX_STREAMS_PER_CIRCUIT)
                        .pow_rend_queue_depth(POW_QUEUE_DEPTH)
                        .enable_pow(true)
                        .build()
                        .expect("We specified a valid nickname");

                    match tor_transport.add_onion_service(
                        onion_service_config,
                        ASB_ONION_SERVICE_PORT,
                        max_concurrent_rend_requests,
                    ) {
                        Ok((addr, handle)) => {
                            tracing::debug!(
                                %addr,
                                "Setting up onion service for libp2p to listen on"
                            );
                            (vec![addr], Some(handle))
                        }
                        Err(err) => {
                            tracing::warn!(error=%err, "Failed to listen on onion address");
                            (vec![], None)
                        }
                    }
                } else {
                    (vec![], None)
                };

                let (wrapped, channels) = WormholeTransport::new(
                    tor_transport,
                    wormhole_max_concurrent_rend_requests,
                    wormhole_num_intro_points,
                );
                (
                    OptionalTransport::some(wrapped),
                    addresses,
                    Some(channels),
                    onion_handle,
                )
            } else {
                (OptionalTransport::none(), vec![], None, None)
            };

        // Build the websocket transport. WsConfig strips the /ws suffix and
        // delegates to its inner TCP+DNS transport for the actual connection.
        let ws_tcp = tcp::tokio::Transport::new(tcp::Config::new().nodelay(true));
        let ws_tcp_dns = dns::tokio::Transport::system(ws_tcp)?;
        let ws_transport = websocket::WsConfig::new(ws_tcp_dns);

        // Build the plain Tor-or-TCP+DNS transport for non-websocket addresses.
        let tcp = maybe_tor_transport
            .or_transport(tcp::tokio::Transport::new(tcp::Config::new().nodelay(true)));
        let tcp_with_dns = dns::tokio::Transport::system(tcp)?;

        // WsConfig only matches addresses ending in /ws or /wss, so it must
        // come first — otherwise Tor or TCP would eagerly claim the address.
        let transport = ws_transport.or_transport(tcp_with_dns).boxed();

        Ok((
            authenticate_and_multiplex(transport, identity)?,
            onion_addresses,
            wormhole_channels,
            onion_service_handle,
        ))
    }
}

pub mod behaviour {
    use std::sync::Arc;

    use libp2p::{connection_limits, identify, identity, ping, swarm::behaviour::toggle::Toggle};
    use swap_p2p::{out_event::alice::OutEvent, patches};

    use crate::network::wormhole;
    use crate::network::wormhole::PeerTrust;
    use crate::network::wormhole::alice::transport::WormholeChannels;

    use super::*;

    /// A `NetworkBehaviour` that represents an XMR/BTC swap node as Alice.
    #[derive(NetworkBehaviour)]
    #[behaviour(out_event = "OutEvent", event_process = false)]
    #[allow(missing_debug_implementations)]
    pub struct Behaviour<LR>
    where
        LR: LatestRate + Send + 'static,
    {
        connection_limits: connection_limits::Behaviour,
        pub rendezvous: Toggle<rendezvous::register::Behaviour>,
        pub quote: quote::Behaviour,
        pub swap_setup: alice::Behaviour<LR>,
        pub transfer_proof: transfer_proof::Behaviour,
        pub cooperative_xmr_redeem: cooperative_xmr_redeem_after_punish::Behaviour,
        pub encrypted_signature: encrypted_signature::Behaviour,
        pub identify: patches::identify::Behaviour,
        pub(crate) wormhole: Toggle<wormhole::alice::Behaviour>,

        /// Ping behaviour that ensures that the underlying network connection
        /// is still alive. If the ping fails a connection close event
        /// will be emitted that is picked up as swarm event.
        ping: ping::Behaviour,
    }

    impl<LR> Behaviour<LR>
    where
        LR: LatestRate + Send + 'static,
    {
        pub fn new(
            min_buy: bitcoin::Amount,
            max_buy: bitcoin::Amount,
            latest_rate: LR,
            resume_only: bool,
            env_config: env::Config,
            identify_params: (identity::Keypair, XmrBtcNamespace),
            rendezvous_nodes: Vec<PeerId>,
            connection_limits: connection_limits::ConnectionLimits,
            trust_provider: Arc<dyn PeerTrust + Send + Sync>,
            wormhole_channels: Option<WormholeChannels>,
            wormhole_swap_freshness_hours: u64,
        ) -> Self {
            let (identity, namespace) = identify_params;
            let agent_version = format!("asb/{} ({})", env!("CARGO_PKG_VERSION"), namespace);
            let protocol_version = "/comit/xmr/btc/1.0.0".to_string();

            let identifyConfig = identify::Config::new(protocol_version, identity.public())
                .with_agent_version(agent_version);

            let pingConfig = ping::Config::new().with_timeout(Duration::from_secs(60));

            let wormhole = wormhole_channels.map(|channels| {
                wormhole::alice::Behaviour::new(
                    &identity,
                    trust_provider,
                    channels.service_tx,
                    channels.handle_rx,
                    wormhole::alice::Config {
                        swap_freshness: Duration::from_secs(
                            wormhole_swap_freshness_hours.saturating_mul(3600),
                        ),
                        ..wormhole::alice::Config::default()
                    },
                )
            });

            let behaviour = if rendezvous_nodes.is_empty() {
                None
            } else {
                Some(rendezvous::register::Behaviour::new(
                    identity,
                    rendezvous_nodes,
                    namespace.into(),
                ))
            };

            Self {
                connection_limits: connection_limits::Behaviour::new(connection_limits),
                rendezvous: Toggle::from(behaviour),
                quote: quote::alice(),
                swap_setup: alice::Behaviour::new(
                    min_buy,
                    max_buy,
                    env_config,
                    latest_rate,
                    resume_only,
                ),
                transfer_proof: transfer_proof::alice(),
                encrypted_signature: encrypted_signature::alice(),
                cooperative_xmr_redeem: cooperative_xmr_redeem_after_punish::alice(),
                ping: ping::Behaviour::new(pingConfig),
                identify: patches::identify::Behaviour::new(identifyConfig),
                wormhole: Toggle::from(wormhole),
            }
        }
    }
}
