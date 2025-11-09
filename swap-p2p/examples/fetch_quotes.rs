use anyhow::Result;
use arti_client::{config::TorClientConfigBuilder, TorClient};
use futures::StreamExt;
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::transport::Boxed;
use libp2p::core::upgrade::Version;
use libp2p::multiaddr::Protocol;
use libp2p::swarm::dial_opts::DialOpts;
use libp2p::swarm::NetworkBehaviour;
use libp2p::{dns, tcp};
use libp2p::{identify, noise, ping, request_response};
use libp2p::{identity, yamux, Multiaddr, PeerId, SwarmBuilder, Transport};
use libp2p_tor::{AddressConversion, TorTransport};
use std::sync::Arc;
use std::time::Duration;
use swap_p2p::libp2p_ext::MultiAddrExt;
use swap_p2p::protocols::{quote, rendezvous};
use tor_rtcompat::tokio::TokioRustlsRuntime;

const USE_TOR: bool = true;

#[derive(NetworkBehaviour)]
struct Behaviour {
    rendezvous: rendezvous::discovery::Behaviour,
    ping: ping::Behaviour,
    identify: identify::Behaviour,
    quote: quote::background::Behaviour,
}

fn create_transport(
    identity: &identity::Keypair,
    tor_client: Option<Arc<TorClient<TokioRustlsRuntime>>>,
) -> Result<Boxed<(PeerId, StreamMuxerBox)>> {
    let auth_upgrade = noise::Config::new(identity)?;
    let multiplex_upgrade = yamux::Config::default();

    if let Some(tor_client) = tor_client {
        let transport = TorTransport::from_client(tor_client, AddressConversion::IpAndDns)
            .upgrade(Version::V1)
            .authenticate(auth_upgrade)
            .multiplex(multiplex_upgrade)
            .timeout(Duration::from_secs(60))
            .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
            .boxed();
        Ok(transport)
    } else {
        // TCP with system DNS
        let tcp = tcp::tokio::Transport::new(tcp::Config::default().nodelay(true));
        let tcp_dns = dns::tokio::Transport::system(tcp)?;
        let transport = tcp_dns
            .upgrade(Version::V1)
            .authenticate(auth_upgrade)
            .multiplex(multiplex_upgrade)
            .timeout(Duration::from_secs(60))
            .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
            .boxed();
        Ok(transport)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(
            "info,swap_p2p=trace,fetch_quotes=trace,libp2p_request_response=trace,libp2p_swarm=debug",
        ))
        .try_init();

    let identity = identity::Keypair::generate_ed25519();

    let tor_client_opt = if USE_TOR {
        let config = TorClientConfigBuilder::default().build()?;
        let runtime = TokioRustlsRuntime::current()?;
        let tor_client = TorClient::with_runtime(runtime)
            .config(config)
            .create_bootstrapped()
            .await?;
        Some(Arc::new(tor_client))
    } else {
        None
    };

    let rendezvous_nodes = swap_env::defaults::default_rendezvous_points();
    let rendezvous_nodes_peer_ids = rendezvous_nodes
        .iter()
        .map(|addr| {
            addr.extract_peer_id()
                .expect("Rendezvous node address must contain peer ID")
        })
        .collect();

    let namespace = rendezvous::XmrBtcNamespace::Mainnet;

    let behaviour = Behaviour {
        rendezvous: rendezvous::discovery::Behaviour::new(
            identity.clone(),
            rendezvous_nodes_peer_ids,
            namespace.into(),
        ),
        ping: ping::Behaviour::new(ping::Config::new().with_interval(Duration::from_secs(1))),
        identify: identify::Behaviour::new(identify::Config::new(
            "fetch_quotes/1.0.0".to_string(),
            identity.public(),
        )),
        quote: quote::background::Behaviour::new(),
    };

    let transport = create_transport(&identity, tor_client_opt)?;

    let mut swarm = SwarmBuilder::with_existing_identity(identity)
        .with_tokio()
        .with_other_transport(|_| transport)?
        .with_behaviour(|_| behaviour)?
        .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    for rendezvous_node_addr in rendezvous_nodes {
        tracing::trace!(
            "Dialing rendezvous node address: {:?}",
            rendezvous_node_addr
        );

        swarm.dial(rendezvous_node_addr.clone()).expect(&format!(
            "Failed to dial rendezvous node address {:?}",
            rendezvous_node_addr
        ));

        swarm.add_peer_address(
            rendezvous_node_addr
                .extract_peer_id()
                .expect("Rendezvous node address must contain peer ID"),
            rendezvous_node_addr,
        );
    }

    loop {
        let event = swarm.select_next_some().await;
        // println!("Event: {:?}", event);

        match event {
            libp2p::swarm::SwarmEvent::Behaviour(event) => match event {
                BehaviourEvent::Rendezvous(event) => match event {
                    rendezvous::discovery::Event::DiscoveredPeer { peer_id, address } => {
                        swarm.add_peer_address(peer_id, address);
                        swarm.behaviour_mut().quote.send_request(&peer_id).await;
                    }
                },
                BehaviourEvent::Quote(quote::background::Event::QuoteReceived { peer, quote }) => {
                    println!("==== !!!! GOT QUOTE FROM !!!! ==== {}: {:?}", peer, quote);
                }
                _ => {}
            },
            libp2p::swarm::SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                println!("CONNECTION ESTABLISHED WITH {}", peer_id);
            }
            _ => {}
        }
    }
}
