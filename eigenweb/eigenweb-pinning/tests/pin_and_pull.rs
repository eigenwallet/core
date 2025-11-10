//! Alice pins a message for Bob on Carol's server
//! Bob fetches the list of messages and pulls them
use bytes::Bytes;
use eigenweb_pinning::storage::{MemoryStorage, Storage};
use eigenweb_pinning::UnsignedPinnedMessage;
use libp2p::core::transport::{MemoryTransport, Transport};
use libp2p::core::upgrade;
use libp2p::futures::StreamExt;
use libp2p::swarm::SwarmEvent;
use libp2p::{identity, Multiaddr, SwarmBuilder};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

const TIMEOUT: Duration = Duration::from_secs(30);
const MESSAGE_CONTENT: &str = "Hello Bob from Alice!";

#[tokio::test]
async fn pin_and_fetch_message() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();

    // Create keypairs
    let (alice_keypair, bob_keypair, carol_keypair) =
        (create_keypair(1), create_keypair(2), create_keypair(3));

    let (carol_peer_id, alice_peer_id, bob_peer_id) = (
        carol_keypair.public().to_peer_id(),
        alice_keypair.public().to_peer_id(),
        bob_keypair.public().to_peer_id(),
    );

    // Create storages
    let (alice_storage, bob_storage, carol_storage) = (
        Arc::new(MemoryStorage::new()),
        Arc::new(MemoryStorage::new()),
        MemoryStorage::new(),
    );

    // Create behaviours
    let alice_behaviour = eigenweb_pinning::client::Behaviour::new(
        alice_keypair.clone(),
        vec![carol_peer_id],
        alice_storage,
        TIMEOUT,
    )
    .await;
    let carol_behaviour = eigenweb_pinning::server::Behaviour::new(carol_storage, TIMEOUT);
    let bob_behaviour = eigenweb_pinning::client::Behaviour::new(
        bob_keypair.clone(),
        vec![carol_peer_id],
        bob_storage.clone(),
        TIMEOUT,
    )
    .await;

    // Create swarms
    let mut alice = create_swarm(alice_keypair, alice_behaviour);
    let mut bob = create_swarm(bob_keypair, bob_behaviour);
    let mut carol = create_swarm(carol_keypair, carol_behaviour);

    // Carol needs to listen
    let carol_address: Multiaddr = format!("/memory/1/p2p/{}", carol_peer_id).parse().unwrap();
    carol.listen_on(carol_address.clone()).unwrap();

    // Connect Alice and Bob to Carol
    alice.dial(carol_address.clone()).unwrap();
    bob.dial(carol_address).unwrap();

    // Alice creates and sends a message for Bob
    let message = UnsignedPinnedMessage {
        id: Uuid::new_v4(),
        sender: alice_peer_id,
        receiver: bob_peer_id,
        ttl: 3600,
        priority: 1,
        encrypted_content: Bytes::from(MESSAGE_CONTENT),
    };

    alice.behaviour_mut().pin_message(message);

    // Run event loops until Bob receives the message
    let mut received_messages: Vec<eigenweb_pinning::SignedPinnedMessage> = Vec::new();
    let timeout = tokio::time::sleep(TIMEOUT);
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            _ = &mut timeout => {
                panic!("Test timed out waiting for message delivery");
            }
            _ = carol.select_next_some() => { }
            _ = alice.select_next_some() => { }
            event = bob.select_next_some() => {
                match event {
                    SwarmEvent::Behaviour(
                        eigenweb_pinning::client::Event::IncomingPinnedMessageReceived(hash),
                    ) => {
                        tracing::info!("Bob: received message with hash {:?}", hash);
                        // Fetch the message from storage
                        if let Ok(Some(message)) = bob_storage.get_by_hash(hash).await {
                            received_messages.push(message);
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // Verify the message
    assert_eq!(received_messages.len(), 1, "Expected exactly 1 message");
    let received: &eigenweb_pinning::SignedPinnedMessage = &received_messages[0];
    assert_eq!(received.message().sender, alice_peer_id);
    assert_eq!(received.message().receiver, bob_peer_id);
    assert_eq!(
        received.message().encrypted_content,
        Bytes::from(MESSAGE_CONTENT)
    );

    // Verify signature
    assert!(
        received.verify_with_peer(alice_peer_id),
        "Message signature verification failed"
    );
}

fn create_keypair(seed: u8) -> identity::Keypair {
    let bytes = [seed; 32];
    identity::Keypair::ed25519_from_bytes(bytes).expect("valid keypair")
}

fn create_swarm<B>(keypair: identity::Keypair, behaviour: B) -> libp2p::Swarm<B>
where
    B: libp2p::swarm::NetworkBehaviour,
{
    SwarmBuilder::with_existing_identity(keypair)
        .with_tokio()
        .with_other_transport(|key| {
            let auth_config = libp2p::noise::Config::new(key).unwrap();
            let base = MemoryTransport::default();
            base.upgrade(upgrade::Version::V1)
                .authenticate(auth_config)
                .multiplex(libp2p::yamux::Config::default())
        })
        .unwrap()
        .with_behaviour(|_| behaviour)
        .unwrap()
        .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(TIMEOUT))
        .build()
}
