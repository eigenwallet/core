//! Alice pins a message for Bob on Carol's server
//! Bob fetches the list of messages and pulls them
use bytes::Bytes;
use eigenweb_pinning::storage::MemoryStorage;
use eigenweb_pinning::UnsignedPinnedMessage;
use libp2p::futures::StreamExt;
use libp2p::swarm::dial_opts::{DialOpts, PeerCondition};
use libp2p::swarm::SwarmEvent;
use libp2p::{identity, Multiaddr, SwarmBuilder};
use std::time::Duration;
use uuid::Uuid;

const TIMEOUT: Duration = Duration::from_secs(10);

/// Helper to create deterministic keypairs for testing
fn create_keypair(seed: u8) -> identity::Keypair {
    let bytes = [seed; 32];
    identity::Keypair::ed25519_from_bytes(bytes).expect("valid keypair")
}

#[tokio::test]
async fn pin_and_fetch_message() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();

    // Create keypairs
    let alice_keypair = create_keypair(1);
    let bob_keypair = create_keypair(2);
    let carol_keypair = create_keypair(3);

    let carol_peer_id = carol_keypair.public().to_peer_id();
    let alice_peer_id = alice_keypair.public().to_peer_id();
    let bob_peer_id = bob_keypair.public().to_peer_id();

    // Create server (Carol)
    let storage = MemoryStorage::new();
    let server_behaviour = eigenweb_pinning::server::Behaviour::new(storage, TIMEOUT);

    let mut server = SwarmBuilder::with_existing_identity(carol_keypair)
        .with_tokio()
        .with_tcp(
            libp2p::tcp::Config::default(),
            libp2p::noise::Config::new,
            libp2p::yamux::Config::default,
        )
        .expect("failed to build server transport")
        .with_behaviour(|_| server_behaviour)
        .expect("failed to build server behaviour")
        .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(TIMEOUT))
        .build();

    // Listen on localhost
    let listen_addr: Multiaddr = "/ip4/127.0.0.1/tcp/0".parse().unwrap();
    server.listen_on(listen_addr).unwrap();

    // Wait for server to start listening and get the address
    let server_addr = loop {
        match server.select_next_some().await {
            SwarmEvent::NewListenAddr { address, .. } => {
                tracing::info!("Server listening on {}", address);
                break format!("{}/p2p/{}", address, carol_peer_id)
                    .parse::<Multiaddr>()
                    .unwrap();
            }
            _ => {}
        }
    };

    // Create client (Alice)
    let alice_storage = MemoryStorage::new();
    let alice_behaviour = eigenweb_pinning::client::Behaviour::new(
        alice_keypair.clone(),
        vec![carol_peer_id],
        alice_storage,
        TIMEOUT,
    );

    let mut alice = SwarmBuilder::with_existing_identity(alice_keypair)
        .with_tokio()
        .with_tcp(
            libp2p::tcp::Config::default(),
            libp2p::noise::Config::new,
            libp2p::yamux::Config::default,
        )
        .expect("failed to build alice transport")
        .with_behaviour(|_| alice_behaviour)
        .expect("failed to build alice behaviour")
        .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(TIMEOUT))
        .build();

    // Create client (Bob)
    let bob_storage = MemoryStorage::new();
    let bob_behaviour = eigenweb_pinning::client::Behaviour::new(
        bob_keypair.clone(),
        vec![carol_peer_id],
        bob_storage,
        TIMEOUT,
    );

    let mut bob = SwarmBuilder::with_existing_identity(bob_keypair)
        .with_tokio()
        .with_tcp(
            libp2p::tcp::Config::default(),
            libp2p::noise::Config::new,
            libp2p::yamux::Config::default,
        )
        .expect("failed to build bob transport")
        .with_behaviour(|_| bob_behaviour)
        .expect("failed to build bob behaviour")
        .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(TIMEOUT))
        .build();

    alice.add_peer_address(carol_peer_id, server_addr.clone());
    bob.add_peer_address(carol_peer_id, server_addr.clone());

    // Connect Alice to Carol
    alice
        .dial(
            DialOpts::peer_id(carol_peer_id)
                .condition(PeerCondition::Disconnected)
                .addresses(vec![server_addr.clone()])
                .extend_addresses_through_behaviour()
                .build(),
        )
        .unwrap();
    bob.dial(
        DialOpts::peer_id(carol_peer_id)
            .condition(PeerCondition::Disconnected)
            .addresses(vec![server_addr])
            .extend_addresses_through_behaviour()
            .build(),
    )
    .unwrap();

    // Alice creates and sends a message for Bob
    let message = UnsignedPinnedMessage {
        id: Uuid::new_v4(),
        sender: alice_peer_id,
        receiver: bob_peer_id,
        ttl: 3600,
        priority: 1,
        encrypted_content: Bytes::from("Hello Bob from Alice!"),
    };

    alice.behaviour_mut().pin_message(message);
    tracing::info!("Alice: message queued for Bob");

    // Run event loops until Bob receives the message
    let mut received_messages = Vec::new();
    let timeout = tokio::time::sleep(TIMEOUT);
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            _ = &mut timeout => {
                panic!("Test timed out waiting for message delivery");
            }
            event = server.select_next_some() => {
                if let SwarmEvent::Behaviour(event) = event {
                    tracing::debug!("Server: {:?}", event);
                }
            }
            event = alice.select_next_some() => {
                if let SwarmEvent::Behaviour(event) = event {
                    tracing::debug!("Alice: {:?}", event);
                }
            }
            event = bob.select_next_some() => {
                match event {
                    SwarmEvent::Behaviour(
                        eigenweb_pinning::client::Event::IncomingPinnedMessagesReceived {
                            messages,
                            ..
                        },
                    ) => {
                        if !messages.is_empty() {
                            tracing::info!("Bob: received {} message(s)", messages.len());
                            received_messages.extend(messages);
                            break;
                        }
                    }
                    SwarmEvent::Behaviour(event) => {
                        tracing::debug!("Bob: {:?}", event);
                    }
                    _ => {}
                }
            }
        }
    }

    // Verify the message
    assert_eq!(received_messages.len(), 1, "Expected exactly 1 message");
    let received = &received_messages[0];
    assert_eq!(received.message().sender, alice_peer_id);
    assert_eq!(received.message().receiver, bob_peer_id);
    assert_eq!(
        received.message().encrypted_content,
        Bytes::from("Hello Bob from Alice!")
    );

    // Verify signature
    assert!(
        received.verify_with_peer(alice_peer_id),
        "Message signature verification failed"
    );
}
