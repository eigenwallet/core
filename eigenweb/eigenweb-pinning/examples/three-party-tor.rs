use arti_client::{config::onion_service::OnionServiceConfigBuilder, TorClient};
use bytes::Bytes;
use eigenweb_pinning::storage::{MemoryStorage, Storage};
use eigenweb_pinning::UnsignedPinnedMessage;
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::transport::Boxed;
use libp2p::core::upgrade::Version;
use libp2p::futures::StreamExt;
use libp2p::swarm::SwarmEvent;
use libp2p::{identity, noise, yamux, Multiaddr, PeerId, SwarmBuilder, Transport};
use libp2p_tor::{AddressConversion, TorTransport};
use std::collections::HashMap;
use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncBufReadExt;
use tor_rtcompat::tokio::TokioRustlsRuntime;
use tracing::{error, info};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::FmtSubscriber;
use uuid::Uuid;

#[derive(Debug, Clone, Copy)]
enum Party {
    Alice,
    Bob,
    Carol,
    David,
}

impl Party {
    fn keypair(&self) -> identity::Keypair {
        let bytes = match self {
            Party::Alice => [1u8; 32],
            Party::Bob => [2u8; 32],
            Party::Carol => [3u8; 32],
            Party::David => [4u8; 32],
        };
        identity::Keypair::ed25519_from_bytes(bytes).expect("valid keypair")
    }

    fn peer_id(&self) -> PeerId {
        self.keypair().public().to_peer_id()
    }

    fn name(&self) -> &'static str {
        match self {
            Party::Alice => "Alice",
            Party::Bob => "Bob",
            Party::Carol => "Carol",
            Party::David => "David",
        }
    }
}

/// Create a Tor client
async fn create_tor_client(data_dir: &PathBuf) -> Result<TorClient<TokioRustlsRuntime>, Box<dyn Error>> {
    let tor_dir = data_dir.join("tor");
    let state_dir = tor_dir.join("state");
    let cache_dir = tor_dir.join("cache");

    // Workaround for https://gitlab.torproject.org/tpo/core/arti/-/issues/2224
    let guards_file = state_dir.join("state").join("guards.json");
    let _ = tokio::fs::remove_file(&guards_file).await;

    let config = arti_client::config::TorClientConfigBuilder::from_directories(state_dir, cache_dir)
        .build()
        .expect("Valid Tor client config");

    let runtime = TokioRustlsRuntime::current().expect("Running with tokio runtime");

    let tor_client = TorClient::with_runtime(runtime)
        .config(config)
        .create_unbootstrapped_async()
        .await?;

    Ok(tor_client)
}

/// Bootstrap a Tor client
async fn bootstrap_tor_client(
    tor_client: Arc<TorClient<TokioRustlsRuntime>>,
) -> Result<(), Box<dyn Error>> {
    info!("Bootstrapping Tor client...");
    tor_client.bootstrap().await?;
    info!("Tor client bootstrapped successfully");
    Ok(())
}

/// Create a transport with Tor support and optionally register an onion service
fn create_transport_with_tor(
    identity: &identity::Keypair,
    tor_client: Arc<TorClient<TokioRustlsRuntime>>,
    register_onion_service: bool,
    port: u16,
) -> Result<(Boxed<(PeerId, StreamMuxerBox)>, Option<Multiaddr>), Box<dyn Error>> {
    let mut tor_transport = TorTransport::from_client(tor_client, AddressConversion::DnsOnly);

    let onion_address = if register_onion_service {
        // Derive nickname from peer id
        let onion_service_config = OnionServiceConfigBuilder::default()
            .nickname(
                identity
                    .public()
                    .to_peer_id()
                    .to_base58()
                    .to_ascii_lowercase()
                    .parse()
                    .expect("Peer ID to be valid nickname"),
            )
            .num_intro_points(3)
            .build()
            .expect("Valid onion service config");

        match tor_transport.add_onion_service(onion_service_config, port) {
            Ok(addr) => {
                info!(%addr, "Onion service configured");
                Some(addr)
            }
            Err(err) => {
                error!(%err, "Failed to configure onion service");
                None
            }
        }
    } else {
        None
    };

    let auth_upgrade = noise::Config::new(identity)?;
    let multiplex_upgrade = yamux::Config::default();

    let transport = tor_transport
        .boxed()
        .upgrade(Version::V1)
        .authenticate(auth_upgrade)
        .multiplex(multiplex_upgrade)
        .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
        .boxed();

    Ok((transport, onion_address))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage:");
        eprintln!("  Server: {} --carol|--david", args[0]);
        eprintln!("  Client: {} --alice|--bob [--carol-addr ONION_ADDR] [--david-addr ONION_ADDR]", args[0]);
        eprintln!();
        eprintln!("Examples:");
        eprintln!("  Start Carol's server:  {} --carol", args[0]);
        eprintln!("  Start David's server:  {} --david", args[0]);
        eprintln!("  Start Alice (both servers): {} --alice --carol-addr <addr1> --david-addr <addr2>", args[0]);
        eprintln!("  Start Alice (Carol only): {} --alice --carol-addr <addr1>", args[0]);
        eprintln!("  Start Bob (David only):   {} --bob --david-addr <addr2>", args[0]);
        std::process::exit(1);
    }

    let party = match args[1].as_str() {
        "--alice" => Party::Alice,
        "--bob" => Party::Bob,
        "--carol" => Party::Carol,
        "--david" => Party::David,
        _ => {
            eprintln!("Invalid argument. Use --alice, --bob, --carol, or --david");
            std::process::exit(1);
        }
    };

    // Parse server addresses for clients
    let mut carol_addr: Option<Multiaddr> = None;
    let mut david_addr: Option<Multiaddr> = None;

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--carol-addr" => {
                if i + 1 < args.len() {
                    carol_addr = Some(args[i + 1].parse()?);
                    i += 2;
                } else {
                    eprintln!("--carol-addr requires an address argument");
                    std::process::exit(1);
                }
            }
            "--david-addr" => {
                if i + 1 < args.len() {
                    david_addr = Some(args[i + 1].parse()?);
                    i += 2;
                } else {
                    eprintln!("--david-addr requires an address argument");
                    std::process::exit(1);
                }
            }
            _ => {
                eprintln!("Unknown argument: {}", args[i]);
                std::process::exit(1);
            }
        }
    }

    // Initialize tracing
    init_tracing(LevelFilter::TRACE);

    info!("Starting as {}", party.name());
    info!("Local peer id: {}", party.peer_id());

    match party {
        Party::Carol | Party::David => run_server(party).await,
        Party::Alice => run_client(party, Party::Bob, carol_addr, david_addr).await,
        Party::Bob => run_client(party, Party::Alice, carol_addr, david_addr).await,
    }
}

async fn run_server(party: Party) -> Result<(), Box<dyn Error>> {
    let keypair = party.keypair();
    let data_dir = std::env::temp_dir().join(format!("eigenweb-pinning-{}", party.name()));

    info!("Starting pinning server as {}", party.name());
    info!("Data directory: {:?}", data_dir);

    // Create and bootstrap Tor client
    let tor_client: Arc<TorClient<TokioRustlsRuntime>> = Arc::new(create_tor_client(&data_dir).await?);
    bootstrap_tor_client(tor_client.clone()).await?;

    // Create transport with onion service
    let (transport, onion_address) = create_transport_with_tor(&keypair, tor_client, true, 999)?;

    if let Some(ref addr) = onion_address {
        info!("Onion service will be available at: {}", addr);
    }

    // Create the pinning server behaviour
    let storage = MemoryStorage::new();
    let behaviour = eigenweb_pinning::server::Behaviour::new(storage, Duration::from_secs(60));

    // Build the swarm with the Tor transport
    let mut swarm = SwarmBuilder::with_existing_identity(keypair)
        .with_tokio()
        .with_other_transport(|_| transport)?
        .with_behaviour(|_| behaviour)?
        .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(Duration::from_secs(60 * 10)))
        .build();

    // Listen on the onion address
    if let Some(addr) = onion_address {
        swarm.listen_on(addr.clone())?;
        info!("Waiting for onion service to be published...");
    } else {
        error!("Failed to create onion service, cannot listen");
        return Err("Failed to create onion service".into());
    }

    // Event loop
    loop {
        match swarm.select_next_some().await {
            SwarmEvent::NewListenAddr { address, .. } => {
                info!("LISTENING ON: {}", address);
                info!("===================================================");
                info!("Clients can connect using this address:");
                info!("  {}", address);
                info!("===================================================");
            }
            SwarmEvent::Behaviour(event) => {
                info!("Behaviour event: {:?}", event);
            }
            SwarmEvent::ConnectionEstablished {
                peer_id,
                endpoint,
                num_established,
                ..
            } => {
                info!(
                    "Connection established with {} at {:?} (total: {})",
                    peer_id, endpoint, num_established
                );
            }
            SwarmEvent::ConnectionClosed {
                peer_id,
                cause,
                num_established,
                ..
            } => {
                info!(
                    "Connection closed with {} (remaining: {}) - cause: {:?}",
                    peer_id, num_established, cause
                );
            }
            SwarmEvent::IncomingConnection { send_back_addr, .. } => {
                info!("Incoming connection from {}", send_back_addr);
            }
            SwarmEvent::IncomingConnectionError {
                send_back_addr,
                error,
                ..
            } => {
                error!(
                    "Incoming connection error from {}: {}",
                    send_back_addr, error
                );
            }
            event => {
                info!("Other swarm event: {:?}", event);
            }
        }
    }
}

async fn run_client(
    party: Party,
    other_party: Party,
    carol_addr: Option<Multiaddr>,
    david_addr: Option<Multiaddr>,
) -> Result<(), Box<dyn Error>> {
    let keypair = party.keypair();
    let peer_id = party.peer_id();
    let data_dir = std::env::temp_dir().join(format!("eigenweb-pinning-{}", party.name()));

    info!("Starting as {}", party.name());
    info!("Data directory: {:?}", data_dir);

    // Check that at least one server address is provided
    if carol_addr.is_none() && david_addr.is_none() {
        error!("At least one server address must be provided (--carol-addr or --david-addr)");
        std::process::exit(1);
    }

    // Create the pinning client behaviour
    // Note: We don't use peer IDs for server verification in this example
    let storage = Arc::new(MemoryStorage::new());
    let server_peer_ids = vec![];  // Empty - we're connecting by onion address only
    let behaviour = eigenweb_pinning::client::Behaviour::new(
        keypair.clone(),
        server_peer_ids,
        storage.clone(),
        Duration::from_secs(10),
    )
    .await;

    // Create and bootstrap Tor client
    let tor_client: Arc<TorClient<TokioRustlsRuntime>> = Arc::new(create_tor_client(&data_dir).await?);
    bootstrap_tor_client(tor_client.clone()).await?;

    // Create transport (no onion service for clients)
    let (transport, _) = create_transport_with_tor(&keypair, tor_client, false, 0)?;

    // Build the swarm with the Tor transport
    let mut swarm = SwarmBuilder::with_existing_identity(keypair)
        .with_tokio()
        .with_other_transport(|_| transport)?
        .with_behaviour(|_| behaviour)?
        .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(Duration::from_secs(60 * 10)))
        .build();

    // Connect to the servers
    if let Some(addr) = &carol_addr {
        info!("Dialing Carol's server at {}", addr);
        swarm.dial(addr.clone())?;
    } else {
        info!("Not connecting to Carol's server (no address provided)");
    }

    if let Some(addr) = &david_addr {
        info!("Dialing David's server at {}", addr);
        swarm.dial(addr.clone())?;
    } else {
        info!("Not connecting to David's server (no address provided)");
    }

    // Set up stdin reader for interactive input
    let stdin = tokio::io::stdin();
    let mut stdin_lines = tokio::io::BufReader::new(stdin).lines();
    let other_peer_id = other_party.peer_id();
    let other_name = other_party.name();

    let mut all_messages: HashMap<
        eigenweb_pinning::signature::MessageHash,
        eigenweb_pinning::SignedPinnedMessage,
    > = HashMap::new();

    let mut connected_to_server = false;

    // Event loop
    loop {
        tokio::select! {
            // Handle user input
            line = stdin_lines.next_line() => {
                match line {
                    Ok(Some(input)) => {
                        if input.trim().is_empty() {
                            continue;
                        }

                        let message = UnsignedPinnedMessage {
                            id: Uuid::new_v4(),
                            sender: peer_id,
                            receiver: other_peer_id,
                            ttl: 3600, // 1 hour
                            priority: 1,
                            encrypted_content: Bytes::from(input.trim().to_string()),
                        };

                        swarm.behaviour_mut().pin_message(message);
                        info!("Message queued for {}", other_name);
                    }
                    Ok(None) => {
                        info!("EOF on stdin, exiting");
                        return Ok(());
                    }
                    Err(e) => {
                        error!("Error reading from stdin: {}", e);
                    }
                }
            }

            // Handle swarm events
            event = swarm.select_next_some() => {
                match event {
                    SwarmEvent::NewListenAddr { address, .. } => {
                        info!("Listening on {}", address);
                    }
                    SwarmEvent::Behaviour(eigenweb_pinning::client::Event::IncomingPinnedMessageReceived(hash)) => {
                        // Fetch the message from storage using the hash
                        match storage.get_by_hash(hash).await {
                            Ok(Some(msg)) => {
                                all_messages.insert(hash, msg);
                                info!("Received message! Total messages: {}", all_messages.len());
                                for msg in all_messages.values() {
                                    let content = String::from_utf8_lossy(&msg.message().encrypted_content);
                                    info!("  From {}: {}", other_name, content);
                                }
                            }
                            Ok(None) => {
                                error!("Message with hash {:?} not found in storage", hash);
                            }
                            Err(e) => {
                                error!("Error fetching message from storage: {}", e);
                            }
                        }
                    }
                    SwarmEvent::ConnectionEstablished {
                        peer_id: connected_peer,
                        endpoint,
                        num_established,
                        ..
                    } => {
                        info!(
                            "Connection established with {} at {:?} (total: {})",
                            connected_peer, endpoint, num_established
                        );

                        // Check if we connected to a server
                        let is_server = carol_addr.as_ref().map_or(false, |addr| {
                            endpoint.get_remote_address().to_string().contains(&addr.to_string())
                        }) || david_addr.as_ref().map_or(false, |addr| {
                            endpoint.get_remote_address().to_string().contains(&addr.to_string())
                        });

                        if is_server || !connected_to_server {
                            connected_to_server = true;
                            info!("Connected to a pinning server!");
                            info!("Type messages for {} and press Enter", other_name);
                        }
                    }
                    SwarmEvent::ConnectionClosed {
                        peer_id: closed_peer,
                        cause,
                        num_established,
                        ..
                    } => {
                        info!(
                            "Connection closed with {} (remaining: {}) - cause: {:?}",
                            closed_peer, num_established, cause
                        );
                    }
                    SwarmEvent::OutgoingConnectionError { peer_id: failed_peer, error, .. } => {
                        error!("Outgoing connection error to {:?}: {}", failed_peer, error);
                    }
                    SwarmEvent::Dialing {
                        peer_id: dialing_peer,
                        connection_id,
                    } => {
                        info!("Dialing {:?} (connection: {:?})", dialing_peer, connection_id);
                    }
                    event => {
                        info!("Other swarm event: {:?}", event);
                    }
                }
            }
        }
    }
}

fn init_tracing(level: LevelFilter) {
    if level == LevelFilter::OFF {
        return;
    }

    let is_terminal = atty::is(atty::Stream::Stderr);

    let builder = FmtSubscriber::builder()
        .with_env_filter(build_event_filter_str(&[
            (&["eigenweb_pinning", "swap_p2p"], level),
            (&[env!("CARGO_CRATE_NAME")], level),
            (LIBP2P_CRATES, LevelFilter::INFO),
        ]))
        .with_writer(std::io::stderr)
        .with_ansi(is_terminal)
        .with_target(true);

    builder.init();
}

fn build_event_filter_str(crates_with_filters: &[(&[&str], LevelFilter)]) -> String {
    crates_with_filters
        .iter()
        .flat_map(|(crates, level)| {
            crates
                .iter()
                .map(move |crate_name| format!("{}={}", crate_name, level))
        })
        .collect::<Vec<_>>()
        .join(",")
}

const LIBP2P_CRATES: &[&str] = &[
    "libp2p",
    "libp2p_allow_block_list",
    "libp2p_connection_limits",
    "libp2p_core",
    "libp2p_dns",
    "libp2p_identity",
    // "libp2p_noise",
    "libp2p_ping",
    "libp2p_request_response",
    "libp2p_swarm",
    "libp2p_tcp",
    // "libp2p_yamux",
];
