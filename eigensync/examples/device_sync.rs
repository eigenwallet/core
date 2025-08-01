//! Complete working example of device synchronization
//!
//! This example shows two peers (hub and device) synchronizing Automerge documents
//! over libp2p using the simplified eigensync protocol.

use eigensync::{hub, device, DocSync, Request, Response, SyncEvent};
use libp2p::{
    futures::StreamExt,
    identity, noise, tcp, yamux,
    swarm::{NetworkBehaviour, SwarmEvent},
    Transport, SwarmBuilder,
    Swarm, request_response,
};
use std::time::Duration;
use automerge::transaction::Transactable;
use automerge::ReadDoc;
use uuid::Uuid;

#[derive(NetworkBehaviour)]
struct HubBehaviour {
    sync: eigensync::Behaviour,
}

#[derive(NetworkBehaviour)] 
struct DeviceBehaviour {
    sync: eigensync::Behaviour,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    println!("Starting device sync example");

    // Create keypairs
    let hub_key = identity::Keypair::generate_ed25519();
    let device_key = identity::Keypair::generate_ed25519();

    let hub_peer_id = hub_key.public().to_peer_id();
    let device_peer_id = device_key.public().to_peer_id();

    println!("Hub peer ID: {}", hub_peer_id);
    println!("Device peer ID: {}", device_peer_id);

    // Create behaviors
    let hub_behaviour = HubBehaviour { sync: hub() };
    let device_behaviour = DeviceBehaviour { sync: device() };

    // Create document sync instances
    let mut hub_doc_sync = DocSync::new();
    let mut device_doc_sync = DocSync::new();

    // Add some initial data to hub document
    hub_doc_sync.doc_mut().put(automerge::ROOT, "status", "hub_initialized")?;
    hub_doc_sync.doc_mut().put(automerge::ROOT, "timestamp", chrono::Utc::now().timestamp())?;
    
    // Add different data to device document
    device_doc_sync.doc_mut().put(automerge::ROOT, "device_id", "device_001")?;
    device_doc_sync.doc_mut().put(automerge::ROOT, "version", "1.0.0")?;

    // Create swarms using SwarmBuilder
    let mut hub_swarm = SwarmBuilder::with_existing_identity(hub_key.clone())
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_behaviour(|_| Ok(hub_behaviour))?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    let mut device_swarm = SwarmBuilder::with_existing_identity(device_key.clone())
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_behaviour(|_| Ok(device_behaviour))?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    // Start listening on hub
    hub_swarm.listen_on("/ip4/127.0.0.1/tcp/0".parse()?)?;

    // Wait for hub to get an address
    let hub_addr = loop {
        match hub_swarm.select_next_some().await {
            SwarmEvent::NewListenAddr { address, .. } => {
                println!("Hub listening on: {}", address);
                break address;
            }
            _ => {}
        }
    };

    // Connect device to hub
    device_swarm.dial(hub_addr.clone())?;
    println!("Device dialing hub at: {}", hub_addr);
    
    // Add the hub's address to the device's swarm so it knows where to send requests
    device_swarm.add_peer_address(hub_peer_id, hub_addr.clone());

    // Wait for connection
    let mut connected = false;
    let mut synced = false;
    let mut actual_hub_peer_id: Option<libp2p::PeerId> = None;
    let mut connection_attempts = 0;
    let doc_id = Uuid::new_v4();

    println!("Document ID: {}", doc_id);
    println!("Starting sync process...");

    // Main event loop
    loop {
        tokio::select! {
            event = hub_swarm.select_next_some() => {
                match event {
                    SwarmEvent::Behaviour(HubBehaviourEvent::Sync(event)) => {
                        let sync_event: SyncEvent = event.into();
                        match sync_event {
                            SyncEvent::IncomingSync { peer, doc_id, sync_msg, channel } => {
                                println!("Hub received sync request from {}", peer);
                                
                                // Apply the sync message
                                let peer_str = peer.to_string();
                                match hub_doc_sync.receive_sync_message(&peer_str, &sync_msg) {
                                    Ok(response_msg) => {
                                        let response = Response::SyncMsg { 
                                            doc_id, 
                                            msg: response_msg 
                                        };
                                        let result = hub_swarm.behaviour_mut().sync.send_response(channel, response);
                                        match result {
                                            Ok(()) => println!("Hub sent sync response successfully"),
                                            Err(response) => println!("Failed to send response: {:?}", response),
                                        }
                                    }
                                    Err(e) => {
                                        let response = Response::Error { 
                                            doc_id, 
                                            reason: e.to_string() 
                                        };
                                        let result = hub_swarm.behaviour_mut().sync.send_response(channel, response);
                                        match result {
                                            Ok(()) => println!("Hub sent sync response successfully"),
                                            Err(response) => println!("Failed to send response: {:?}", response),
                                        }
                                    }
                                }
                            }
                            other => println!("Hub sync event: {:?}", other),
                        }
                    }
                    SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } => {
                        println!("Hub connected to {} (expected: {}) via {}", peer_id, device_peer_id, endpoint.get_remote_address());
                    }
                    SwarmEvent::ConnectionClosed { peer_id, num_established, .. } => {
                        println!("Hub connection closed to {} (established: {})", peer_id, num_established);
                    }
                    SwarmEvent::NewListenAddr { address, .. } => {
                        println!("Hub listening on: {}", address);
                    }
                    _ => {}
                }
            }
            
            event = device_swarm.select_next_some() => {
                match event {
                    SwarmEvent::Behaviour(DeviceBehaviourEvent::Sync(event)) => {
                        let sync_event: SyncEvent = event.into();
                        match sync_event {
                            SyncEvent::SyncResponse { doc_id, msg } => {
                                println!("Device received sync response for {}", doc_id);
                                
                                if let Some(response_msg) = msg {
                                    let peer_str = hub_peer_id.to_string();
                                    if let Err(e) = device_doc_sync.receive_sync_message(&peer_str, &response_msg) {
                                        eprintln!("Device failed to apply sync: {}", e);
                                    } else {
                                        println!("Device successfully synced!");
                                        synced = true;
                                    }
                                }
                            }
                            SyncEvent::SyncError { doc_id, reason } => {
                                eprintln!("Device sync error for {}: {}", doc_id, reason);
                            }
                            other => println!("Device sync event: {:?}", other),
                        }
                    }
                    SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } => {
                        println!("Device connected to {} (expected: {}) via {}", peer_id, hub_peer_id, endpoint.get_remote_address());
                        actual_hub_peer_id = Some(peer_id);
                        connected = true;
                        connection_attempts = 0; // Reset attempts on successful connection
                        
                        // Wait a bit before sending the request to ensure connection is stable
                        println!("Device connection established, waiting before sending request...");
                    }
                    SwarmEvent::ConnectionClosed { peer_id, num_established, connection_id, endpoint, cause } => {
                        println!("Device connection closed to {} (established: {}) (connection_id: {}) (cause: {:?})", peer_id, num_established, connection_id, cause);
                        if num_established == 0 {
                            connected = false;
                            actual_hub_peer_id = None;
                        }
                    }

                    _ => {}
                }
            }

            // Trigger sync after connection
            _ = tokio::time::sleep(Duration::from_secs(5)) => {
                if connected && !synced {
                    if let Some(actual_hub_id) = actual_hub_peer_id {
                        let peer_str = actual_hub_id.to_string();
                        if let Some(sync_msg) = device_doc_sync.generate_sync_message(&peer_str) {
                            let request = Request { doc_id, sync_msg };
                            
                            println!("Device sending sync request to {}", actual_hub_id);
                            device_swarm.behaviour_mut().sync.send_request(&actual_hub_id, request);
                        } else {
                            println!("No sync message to send");
                        }
                    } else {
                        println!("No hub peer ID available");
                    }
                } else if !connected {
                    // Try to reconnect if not connected
                    connection_attempts += 1;
                    if connection_attempts <= 3 {
                        println!("Attempting to reconnect to hub (attempt {})", connection_attempts);
                        device_swarm.dial(hub_addr.clone())?;
                    } else {
                        println!("Failed to establish connection after {} attempts", connection_attempts);
                        break;
                    }
                }
                
                if synced {
                    break;
                }
            }
        }
    }

    // Print final state
    println!("\nSync completed! Final document states:");
    
    println!("\nHub document:");
    print_document_contents(hub_doc_sync.doc());
    
    println!("\nDevice document:");
    print_document_contents(device_doc_sync.doc());

    // Verify sync worked
    let hub_keys: Vec<_> = hub_doc_sync.doc().keys(automerge::ROOT).collect();
    let device_keys: Vec<_> = device_doc_sync.doc().keys(automerge::ROOT).collect();
    
    println!("\nHub has {} keys, Device has {} keys", hub_keys.len(), device_keys.len());
    
    if hub_keys.len() == device_keys.len() && hub_keys.len() == 4 {
        println!("Sync verification successful!");
    } else {
        println!("Sync verification failed");
    }

    Ok(())
}

fn print_document_contents(doc: &automerge::AutoCommit) {
    for key in doc.keys(automerge::ROOT) {
        if let Ok(Some((value, _))) = doc.get(automerge::ROOT, &key) {
            println!("  {}: {}", key, value);
        }
    }
} 