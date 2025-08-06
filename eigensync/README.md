# Eigensync

A minimal distributed document synchronization system built on Automerge CRDTs and libp2p networking.

## Quick Start

```rust
use eigensync::{hub, device, DocSync};
use libp2p::{identity, swarm::NetworkBehaviour, Swarm};
use automerge::transaction::Transactable;
use uuid::Uuid;

#[derive(NetworkBehaviour)]
struct SyncBehaviour {
    sync: eigensync::Behaviour,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create keypairs
    let hub_key = identity::Keypair::generate_ed25519();
    let device_key = identity::Keypair::generate_ed25519();

    // Create behaviors 
    let hub_behaviour = SyncBehaviour { sync: hub() };
    let device_behaviour = SyncBehaviour { sync: device() };

    // Create document sync instances
    let mut hub_doc = DocSync::new();
    let mut device_doc = DocSync::new();

    // Add data to documents
    hub_doc.doc_mut().put(automerge::ROOT, "greeting", "Hello from hub!")?;
    device_doc.doc_mut().put(automerge::ROOT, "status", "ready")?;

    // Create swarms and sync...
    // See examples/device_sync.rs for complete implementation

    Ok(())
}
```

## Features

- **Simple Protocol**: Single libp2p request-response protocol with CBOR encoding
- **Pure Automerge**: Clean integration with Automerge's built-in sync primitives  
- **Minimal API**: Just `hub()`, `device()`, and `DocSync` - no complex configurations
- **P2P Networking**: Direct peer-to-peer communication using libp2p

## API

### Creating Behaviors

```rust
use eigensync::{hub, device};

// For a hub (accepts inbound sync requests)
let hub_behaviour = hub();

// For a device (makes outbound sync requests)
let device_behaviour = device();
```

### Document Synchronization

```rust
use eigensync::DocSync;
use automerge::transaction::Transactable;

let mut doc_sync = DocSync::new();

// Add data to document
doc_sync.doc_mut().put(automerge::ROOT, "key", "value")?;

// Generate sync message for a peer
if let Some(sync_msg) = doc_sync.generate_sync_message("peer_id") {
    // Send sync_msg to peer via libp2p
}

// Apply sync message from a peer
if let Ok(response) = doc_sync.receive_sync_message("peer_id", &received_msg) {
    // Optionally send response back to peer
}
```

### Event Handling

```rust
use eigensync::SyncEvent;

match sync_event {
    SyncEvent::IncomingSync { peer, doc_id, sync_msg, channel } => {
        // Hub receives sync request
        // Apply sync_msg and send response via channel
    }
    SyncEvent::SyncResponse { doc_id, msg } => {
        // Device receives sync response
        // Apply response message if present
    }
    SyncEvent::SyncError { doc_id, reason } => {
        // Handle sync error
    }
    SyncEvent::OutboundFailure { peer, error } => {
        // Handle connection failure
    }
}
```

## Running the Example

```bash
cd eigensync
cargo run --example device_sync
```

This will start two peers (hub and device) that:
1. Create documents with different initial data
2. Connect via libp2p 
3. Synchronize their documents using Automerge CRDTs
4. Verify that both documents now contain all data

## Architecture

The system consists of just three main components:

- **Protocol** (`protocol.rs`): Single libp2p request-response protocol
- **Sync** (`sync.rs`): Pure Automerge document synchronization logic
- **API** (`lib.rs`): Minimal public interface

No complex server/client architecture, no database dependencies, no feature flags. Just simple, direct document synchronization. 