use crate::network::rendezvous;
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

    use arti_client::{config::onion_service::OnionServiceConfigBuilder, TorClient};
    use libp2p::{core::transport::OptionalTransport, dns, identity, tcp, Transport};
    use libp2p_tor::AddressConversion;
    use tor_rtcompat::tokio::TokioRustlsRuntime;

    use super::*;

    static ASB_ONION_SERVICE_NICKNAME: &str = "asb";
    static ASB_ONION_SERVICE_PORT: u16 = 9939;

    type OnionTransportWithAddresses = (Boxed<(PeerId, StreamMuxerBox)>, Vec<Multiaddr>);

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
    ) -> Result<OnionTransportWithAddresses> {
        let (maybe_tor_transport, onion_addresses) = if let Some(tor_client) = maybe_tor_client {
            let mut tor_transport =
                libp2p_tor::TorTransport::from_client(tor_client, AddressConversion::DnsOnly);

            let addresses = if register_hidden_service {
                let onion_service_config = OnionServiceConfigBuilder::default()
                    .nickname(
                        ASB_ONION_SERVICE_NICKNAME
                            .parse()
                            .expect("Static nickname to be valid"),
                    )
                    .num_intro_points(num_intro_points)
                    .build()
                    .expect("We specified a valid nickname");

                match tor_transport.add_onion_service(onion_service_config, ASB_ONION_SERVICE_PORT)
                {
                    Ok(addr) => {
                        tracing::debug!(
                            %addr,
                            "Setting up onion service for libp2p to listen on"
                        );
                        vec![addr]
                    }
                    Err(err) => {
                        tracing::warn!(error=%err, "Failed to listen on onion address");
                        vec![]
                    }
                }
            } else {
                vec![]
            };

            (OptionalTransport::some(tor_transport), addresses)
        } else {
            (OptionalTransport::none(), vec![])
        };

        let tcp = maybe_tor_transport
            .or_transport(tcp::tokio::Transport::new(tcp::Config::new().nodelay(true)));
        let tcp_with_dns = dns::tokio::Transport::system(tcp)?;

        Ok((
            authenticate_and_multiplex(tcp_with_dns.boxed(), identity)?,
            onion_addresses,
        ))
    }
}

pub mod behaviour {
    use libp2p::{identify, identity, ping, swarm::behaviour::toggle::Toggle};
    use swap_p2p::out_event::alice::OutEvent;

    use super::{rendezvous::register, *};

    /// A `NetworkBehaviour` that represents an XMR/BTC swap node as Alice.
    #[derive(NetworkBehaviour)]
    #[behaviour(out_event = "OutEvent", event_process = false)]
    #[allow(missing_debug_implementations)]
    pub struct Behaviour<LR>
    where
        LR: LatestRate + Send + 'static,
    {
        pub rendezvous: Toggle<rendezvous::register::Behaviour>,
        pub quote: quote::Behaviour,
        pub swap_setup: alice::Behaviour<LR>,
        pub transfer_proof: transfer_proof::Behaviour,
        pub cooperative_xmr_redeem: cooperative_xmr_redeem_after_punish::Behaviour,
        pub encrypted_signature: encrypted_signature::Behaviour,
        pub identify: identify::Behaviour,

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
            rendezvous_nodes: Vec<register::RendezvousNode>,
        ) -> Self {
            let (identity, namespace) = identify_params;
            let agent_version = format!("asb/{} ({})", env!("CARGO_PKG_VERSION"), namespace);
            let protocol_version = "/comit/xmr/btc/1.0.0".to_string();

            let identifyConfig = identify::Config::new(protocol_version, identity.public())
                .with_agent_version(agent_version);

            let pingConfig = ping::Config::new().with_timeout(Duration::from_secs(60));

            let behaviour = if rendezvous_nodes.is_empty() {
                None
            } else {
                Some(rendezvous::register::Behaviour::new(
                    identity,
                    rendezvous_nodes,
                ))
            };

            Self {
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
                identify: identify::Behaviour::new(identifyConfig),
            }
        }
    }
}
