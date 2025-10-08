use bytes::Bytes;
use eigenweb_pinning::storage::MemoryStorage;
use eigenweb_pinning::{signature::SignedMessage, UnsignedPinnedMessage};
use libp2p::futures::StreamExt;
use libp2p::swarm::SwarmEvent;
use libp2p::{identity, Multiaddr, PeerId, SwarmBuilder};
use std::error::Error;
use std::time::Duration;
use tokio::io::AsyncBufReadExt;
use tracing::{error, info};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::FmtSubscriber;
use uuid::Uuid;

#[derive(Debug, Clone, Copy)]
enum Party {
    Alice,
    Bob,
    Carol,
}

impl Party {
    fn keypair(&self) -> identity::Keypair {
        let bytes = match self {
            Party::Alice => [1u8; 32],
            Party::Bob => [2u8; 32],
            Party::Carol => [3u8; 32],
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
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();
    let party = if args.len() < 2 {
        eprintln!("Usage: {} --alice|--bob|--carol", args[0]);
        eprintln!("  --alice: Run as Alice (sender client)");
        eprintln!("  --bob:   Run as Bob (receiver client)");
        eprintln!("  --carol: Run as Carol (pinning server)");
        std::process::exit(1);
    } else {
        match args[1].as_str() {
            "--alice" => Party::Alice,
            "--bob" => Party::Bob,
            "--carol" => Party::Carol,
            _ => {
                eprintln!("Invalid argument. Use --alice, --bob, or --carol");
                std::process::exit(1);
            }
        }
    };

    // Initialize tracing
    init_tracing(LevelFilter::DEBUG);

    info!("Starting as {}", party.name());
    info!("Local peer id: {}", party.peer_id());

    match party {
        Party::Carol => run_server(party).await,
        Party::Alice => run_client_alice(party).await,
        Party::Bob => run_client_bob(party).await,
    }
}

async fn run_server(party: Party) -> Result<(), Box<dyn Error>> {
    let keypair = party.keypair();
    let peer_id = party.peer_id();

    info!("Starting pinning server as {}", party.name());

    // Create the pinning server behaviour
    let storage = MemoryStorage::new();
    let behaviour = eigenweb_pinning::server::Behaviour::new(storage, Duration::from_secs(60));

    // Build the swarm
    let mut swarm = SwarmBuilder::with_existing_identity(keypair)
        .with_tokio()
        .with_tcp(
            libp2p::tcp::Config::default(),
            libp2p::noise::Config::new,
            libp2p::yamux::Config::default,
        )?
        .with_behaviour(|_| behaviour)?
        .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(Duration::from_secs(60 * 10)))
        .build();

    // Listen on a fixed port for easier connection
    let listen_addr: Multiaddr = "/ip4/0.0.0.0/tcp/9000".parse()?;
    swarm.listen_on(listen_addr.clone())?;
    info!("Server listening on {}", listen_addr);

    // Event loop
    loop {
        match swarm.select_next_some().await {
            SwarmEvent::NewListenAddr { address, .. } => {
                info!("✓ Listening on {}", address);
                info!(
                    "Clients can connect using: /ip4/127.0.0.1/tcp/9000/p2p/{}",
                    peer_id
                );
            }
            SwarmEvent::Behaviour(event) => {
                info!("📨 Behaviour event: {:?}", event);
            }
            SwarmEvent::ConnectionEstablished {
                peer_id,
                endpoint,
                num_established,
                ..
            } => {
                info!(
                    "🔗 Connection established with {} at {:?} (total: {})",
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
                    "🔌 Connection closed with {} (remaining: {}) - cause: {:?}",
                    peer_id, num_established, cause
                );
            }
            SwarmEvent::IncomingConnection { send_back_addr, .. } => {
                info!("📞 Incoming connection from {}", send_back_addr);
            }
            SwarmEvent::IncomingConnectionError {
                send_back_addr,
                error,
                ..
            } => {
                error!(
                    "❌ Incoming connection error from {}: {}",
                    send_back_addr, error
                );
            }
            event => {
                info!("Other swarm event: {:?}", event);
            }
        }
    }
}

async fn run_client_alice(party: Party) -> Result<(), Box<dyn Error>> {
    let keypair = party.keypair();
    let peer_id = party.peer_id();

    info!("Starting as Alice (sender)");

    // Create the pinning client behaviour
    let storage = MemoryStorage::new();
    let carol_peer_id = Party::Carol.peer_id();
    let behaviour = eigenweb_pinning::client::Behaviour::new(
        peer_id,
        vec![carol_peer_id],
        storage,
        Duration::from_secs(10),
    );

    // Build the swarm
    let mut swarm = SwarmBuilder::with_existing_identity(keypair.clone())
        .with_tokio()
        .with_tcp(
            libp2p::tcp::Config::default(),
            libp2p::noise::Config::new,
            libp2p::yamux::Config::default,
        )?
        .with_behaviour(|_| behaviour)?
        .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(Duration::from_secs(60 * 10)))
        .build();

    // Listen on a random port
    let listen_addr: Multiaddr = "/ip4/0.0.0.0/tcp/0".parse()?;
    swarm.listen_on(listen_addr)?;

    // Connect to Carol's server
    let server_addr: Multiaddr =
        format!("/ip4/127.0.0.1/tcp/9000/p2p/{}", carol_peer_id).parse()?;

    info!("🔌 Dialing Carol's server at {}", server_addr);
    swarm.dial(server_addr)?;

    // Set up stdin reader for interactive input
    let stdin = tokio::io::stdin();
    let mut stdin_lines = tokio::io::BufReader::new(stdin).lines();
    let bob_peer_id = Party::Bob.peer_id();

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

                        if !connected_to_server {
                            info!("⚠️  Not yet connected to Carol's server. Please wait...");
                            continue;
                        }

                        // Create a new message for Bob
                        let message = UnsignedPinnedMessage {
                            id: Uuid::new_v4(),
                            sender: peer_id,
                            receiver: bob_peer_id,
                            ttl: 3600, // 1 hour
                            priority: 1,
                            encrypted_content: Bytes::from(input.trim().to_string()),
                        };

                        // Sign the message
                        match SignedMessage::new(&keypair, message) {
                            Ok(signed_message) => {
                                let hash = signed_message.content_hash();
                                info!("📝 Created message for Bob with hash: {:02x?}", hash);

                                // Pin the message (stores it locally and will be sent via heartbeat)
                                swarm.behaviour_mut().insert_pinned_message(signed_message);
                                info!("✅ Message queued for Bob. It will be sent to Carol's server.");
                            }
                            Err(e) => {
                                error!("❌ Failed to sign message: {}", e);
                            }
                        }
                    }
                    Ok(None) => {
                        info!("EOF on stdin, exiting");
                        break;
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
                        info!("✓ Listening on {}", address);
                    }
                    SwarmEvent::Behaviour(event) => {
                        info!("📨 Behaviour event: {:?}", event);
                    }
                    SwarmEvent::ConnectionEstablished {
                        peer_id: connected_peer,
                        endpoint,
                        num_established,
                        ..
                    } => {
                        info!(
                            "🔗 Connection established with {} at {:?} (total: {})",
                            connected_peer, endpoint, num_established
                        );
                        if connected_peer == carol_peer_id {
                            connected_to_server = true;
                            info!("✨ Connected to Carol's server!");
                            info!("💬 You can now type messages for Bob and press Enter.");
                            info!("   Messages will be automatically pinned to Carol's server.");
                        }
                    }
                    SwarmEvent::ConnectionClosed {
                        peer_id: closed_peer,
                        cause,
                        num_established,
                        ..
                    } => {
                        info!(
                            "🔌 Connect ion closed with {} (remaining: {}) - cause: {:?}",
                            closed_peer, num_established, cause
                        );
                        if closed_peer == carol_peer_id {
                            connected_to_server = false;
                            info!("⚠️  Lost connection to Carol's server");
                        }
                    }
                    SwarmEvent::OutgoingConnectionError { peer_id: failed_peer, error, .. } => {
                        error!("❌ Outgoing connection error to {:?}: {}", failed_peer, error);
                    }
                    SwarmEvent::Dialing {
                        peer_id: dialing_peer,
                        connection_id,
                    } => {
                        info!("📞 Dialing {:?} (connection: {:?})", dialing_peer, connection_id);
                    }
                    event => {
                        info!("Other swarm event: {:?}", event);
                    }
                }
            }
        }
    }

    Ok(())
}

async fn run_client_bob(party: Party) -> Result<(), Box<dyn Error>> {
    let keypair = party.keypair();
    let peer_id = party.peer_id();

    info!("Starting as Bob (receiver)");

    // Create the pinning client behaviour
    let storage = MemoryStorage::new();
    let carol_peer_id = Party::Carol.peer_id();
    let behaviour = eigenweb_pinning::client::Behaviour::new(
        peer_id,
        vec![carol_peer_id],
        storage,
        Duration::from_secs(10),
    );

    // Build the swarm
    let mut swarm = SwarmBuilder::with_existing_identity(keypair)
        .with_tokio()
        .with_tcp(
            libp2p::tcp::Config::default(),
            libp2p::noise::Config::new,
            libp2p::yamux::Config::default,
        )?
        .with_behaviour(|_| behaviour)?
        .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(Duration::from_secs(60 * 10)))
        .build();

    // Listen on a random port
    let listen_addr: Multiaddr = "/ip4/0.0.0.0/tcp/0".parse()?;
    swarm.listen_on(listen_addr)?;

    // Connect to Carol's server
    let server_addr: Multiaddr =
        format!("/ip4/127.0.0.1/tcp/9000/p2p/{}", carol_peer_id).parse()?;

    info!("🔌 Dialing Carol's server at {}", server_addr);
    swarm.dial(server_addr)?;

    let mut connected_to_server = false;
    let mut all_messages = Vec::new();

    // Event loop
    loop {
        tokio::select! {
            // Handle swarm events
            event = swarm.select_next_some() => {
                match event {
                    SwarmEvent::NewListenAddr { address, .. } => {
                        info!("✓ Listening on {}", address);
                    }
                    SwarmEvent::Behaviour(event) => {
                        match event {
                            eigenweb_pinning::client::Event::IncomingPinnedMessagesReceived { peer, outgoing_request_id, messages } => {
                                if messages.is_empty() {
                                    info!("📭 No new messages (request {:?})", outgoing_request_id);
                                }

                                all_messages.extend(messages.into_iter());

                                info!("📬 Received {} message(s) from {} (request {:?})", all_messages.len(), peer, outgoing_request_id);
                                for msg in &all_messages {
                                    let content = String::from_utf8_lossy(&msg.message().encrypted_content);
                                    info!("  💌 From Alice: {}", content);
                                }
                            }
                            _ => {
                                info!("📨 Behaviour event: {:?}", event);
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
                            "🔗 Connection established with {} at {:?} (total: {})",
                            connected_peer, endpoint, num_established
                        );
                        if connected_peer == carol_peer_id {
                            connected_to_server = true;
                            info!("✨ Connected to Carol's server!");
                            info!("📡 The client will automatically fetch and pull messages in the background.");
                        }
                    }
                    SwarmEvent::ConnectionClosed {
                        peer_id: closed_peer,
                        cause,
                        num_established,
                        ..
                    } => {
                        info!(
                            "🔌 Connection closed with {} (remaining: {}) - cause: {:?}",
                            closed_peer, num_established, cause
                        );
                        if closed_peer == carol_peer_id {
                            connected_to_server = false;
                            info!("⚠️  Lost connection to Carol's server");
                        }
                    }
                    SwarmEvent::OutgoingConnectionError { peer_id: failed_peer, error, .. } => {
                        error!("❌ Outgoing connection error to {:?}: {}", failed_peer, error);
                    }
                    SwarmEvent::Dialing {
                        peer_id: dialing_peer,
                        connection_id,
                    } => {
                        info!("📞 Dialing {:?} (connection: {:?})", dialing_peer, connection_id);
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
            (&["eigenweb_pinning"], level),
            (&[env!("CARGO_CRATE_NAME")], level),
            (LIBP2P_CRATES, level),
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
