# eigenweb-pinning Design Document

This document outlines the architecture for the `eigenweb-pinning` protocol based on canonical libp2p patterns.

## Overview

`eigenweb-pinning` is a libp2p protocol for a message-pinning service where users can pin messages at pinning servers for retrieval by other peers. This protocol follows the same patterns used by official libp2p protocols like rendezvous, identify, and request-response.

## References

Key libp2p protocol implementations to study:

- `rust-libp2p/protocols/rendezvous/` - Client/server pattern with state management
- `rust-libp2p/protocols/request-response/` - Base protocol for request/response patterns
- `rust-libp2p/protocols/identify/` - Simple protocol structure
- `rust-libp2p/examples/rendezvous/src/` - Usage examples

## Crate Structure

```
eigenweb-pinning/
├── Cargo.toml
├── src/
│   ├── lib.rs          # Public API, shared types, protocol constants
│   ├── codec.rs        # Message encoding/decoding (CBOR or JSON)
│   ├── client.rs       # client::Behaviour (outbound protocol)
│   └── server.rs       # server::Behaviour (inbound protocol)
```

### lib.rs

Exports public types and constants:

```rust
pub use client::{Behaviour as Client, Event as ClientEvent};
pub use server::{Behaviour as Server, Event as ServerEvent};

// Protocol identifier
pub const PROTOCOL_IDENT: StreamProtocol = StreamProtocol::new("/eigenwallet/pinning/1.0.0");

// Shared message types
pub struct Message {
    pub id: MessageId,
    pub content: Vec<u8>,
    pub timestamp: u64,
}

pub struct PinRequest {
    pub message: Message,
    pub ttl: Option<u64>,
}

pub struct RetrieveRequest {
    pub message_id: MessageId,
}

// Response types
pub enum PinResponse {
    Success { message_id: MessageId },
    Error { reason: String },
}

pub enum RetrieveResponse {
    Found { message: Message },
    NotFound,
}
```

### codec.rs

Implements message serialization. See `rust-libp2p/protocols/rendezvous/src/codec.rs` for reference.

Use either:

- `libp2p_request_response::cbor::Behaviour` for CBOR encoding
- `libp2p_request_response::json::Behaviour` for JSON encoding

Or implement custom codec if needed.

## Client Behaviour

Reference: `rust-libp2p/protocols/rendezvous/src/client.rs`

The client behaviour handles outbound requests to pinning servers.

### Structure

```rust
pub struct Behaviour {
    /// Wraps the request-response protocol
    inner: libp2p_request_response::Behaviour<crate::codec::Codec>,

    /// Tracks outbound pin requests awaiting acknowledgment
    waiting_for_pin_ack: HashMap<OutboundRequestId, Message>,

    /// Tracks outbound retrieve requests awaiting responses
    waiting_for_retrieve: HashMap<OutboundRequestId, MessageId>,

    /// Buffers messages to be pinned when connection is established
    buffered_pins: HashMap<PeerId, Vec<(Message, Option<u64>)>>,

    /// Optional local cache of retrieved messages
    message_cache: HashMap<MessageId, Message>,
}
```

### Key Methods

```rust
impl Behaviour {
    pub fn new() -> Self {
        // Use ProtocolSupport::Outbound - client only sends requests
        let inner = libp2p_request_response::Behaviour::with_codec(
            crate::codec::Codec::default(),
            iter::once((PROTOCOL_IDENT, ProtocolSupport::Outbound)),
            Config::default(),
        );

        Self {
            inner,
            waiting_for_pin_ack: HashMap::new(),
            waiting_for_retrieve: HashMap::new(),
            buffered_pins: HashMap::new(),
            message_cache: HashMap::new(),
        }
    }

    /// Pin a message at a remote pinning server
    pub fn pin_message(
        &mut self,
        peer_id: PeerId,
        message: Message,
        ttl: Option<u64>,
    ) -> PinRequestId {
        let request = PinRequest {
            message: message.clone(),
            ttl,
        };
        let id = self.inner.send_request(&peer_id, request);
        self.waiting_for_pin_ack.insert(id, message);
        id
    }

    /// Request a pinned message from a server
    pub fn retrieve_message(
        &mut self,
        peer_id: PeerId,
        message_id: MessageId,
    ) -> RetrieveRequestId {
        let request = RetrieveRequest { message_id };
        let id = self.inner.send_request(&peer_id, request);
        self.waiting_for_retrieve.insert(id, message_id);
        id
    }
}
```

### Events

```rust
pub enum Event {
    /// Message was successfully pinned at server
    MessagePinned {
        peer: PeerId,
        message_id: MessageId,
        request_id: OutboundRequestId,
    },

    /// Failed to pin message
    PinFailed {
        peer: PeerId,
        message_id: MessageId,
        error: PinError,
    },

    /// Retrieved a pinned message
    MessageRetrieved { peer: PeerId, message: Message },

    /// Message not found at server
    MessageNotFound { peer: PeerId, message_id: MessageId },
}
```

### NetworkBehaviour Implementation

Reference: `rust-libp2p/protocols/rendezvous/src/client.rs:194-300`

The `poll()` method should:

1. Poll the inner request-response behaviour
2. Match on incoming events
3. Update internal state (remove from waiting maps)
4. Emit high-level events to the application

## Server Behaviour

Reference: `rust-libp2p/protocols/rendezvous/src/server.rs`

The server behaviour handles inbound requests from clients.

### Structure

```rust
pub struct Behaviour<S = InMemoryStorage>
where
    S: PinStorage,
{
    /// Wraps the request-response protocol
    inner: libp2p_request_response::Behaviour<crate::codec::Codec>,

    /// Storage backend for persisting pins
    storage: S,

    /// Configuration (max pin size, default TTL, etc.)
    config: Config,
}
```

### Storage Trait

```rust
pub trait PinStorage: Send + Sync {
    /// Store a pinned message
    fn store(
        &self,
        peer_id: PeerId,
        message: Message,
        ttl: u64,
    ) -> impl Future<Output = Result<(), StorageError>>;

    /// Retrieve a pinned message
    fn retrieve(
        &self,
        message_id: MessageId,
    ) -> impl Future<Output = Result<Option<Message>, StorageError>>;

    /// Remove expired pins
    fn cleanup_expired(&self) -> impl Future<Output = Result<usize, StorageError>>;
}

/// Default in-memory implementation
pub struct InMemoryStorage {
    pins: Arc<RwLock<HashMap<MessageId, (Message, Instant)>>>,
}

impl PinStorage for InMemoryStorage {
    // Implementation
}
```

### Key Methods

```rust
impl Behaviour<InMemoryStorage> {
    pub fn new(config: Config) -> Self {
        // Use ProtocolSupport::Inbound - server only handles requests
        let inner = libp2p_request_response::Behaviour::with_codec(
            crate::codec::Codec::default(),
            iter::once((PROTOCOL_IDENT, ProtocolSupport::Inbound)),
            Config::default(),
        );

        Self {
            inner,
            storage: InMemoryStorage::new(),
            config,
        }
    }
}

impl<S: PinStorage> Behaviour<S> {
    pub fn with_storage(storage: S, config: Config) -> Self {
        let inner = libp2p_request_response::Behaviour::with_codec(
            crate::codec::Codec::default(),
            iter::once((PROTOCOL_IDENT, ProtocolSupport::Inbound)),
            Config::default(),
        );

        Self {
            inner,
            storage,
            config,
        }
    }
}
```

### Events

```rust
pub enum Event {
    /// Successfully stored a pin
    PinStored { peer: PeerId, message_id: MessageId },

    /// Failed to store a pin
    PinRejected {
        peer: PeerId,
        message_id: MessageId,
        reason: RejectReason,
    },

    /// Served a retrieve request
    RetrieveServed {
        peer: PeerId,
        message_id: MessageId,
        found: bool,
    },
}
```

### NetworkBehaviour Implementation

Reference: `rust-libp2p/protocols/rendezvous/src/server.rs:117-300`

The `poll()` method should:

1. Poll the inner request-response behaviour
2. On incoming `PinRequest`: validate, call `storage.store()`, send response
3. On incoming `RetrieveRequest`: call `storage.retrieve()`, send response
4. Emit events for monitoring/logging

## Usage in Applications

### Client Usage (swap CLI)

Reference: `rust-libp2p/examples/rendezvous/src/bin/rzv-register.rs`

```rust
use eigenweb_pinning;

#[derive(NetworkBehaviour)]
#[behaviour(to_swarm = "OutEvent")]
pub struct Behaviour {
    pub quote: quote::Behaviour,
    pub transfer_proof: transfer_proof::Behaviour,
    pub pinning: eigenweb_pinning::client::Behaviour,  // Compose it
    pub identify: identify::Behaviour,
    pub ping: ping::Behaviour,
}

// In event loop:
match event {
    SwarmEvent::ConnectionEstablished { peer_id, .. } => {
        // Pin a message directly
        swarm.behaviour_mut().pinning.pin_message(
            peer_id,
            message,
            Some(3600) // 1 hour TTL
        );
    }

    SwarmEvent::Behaviour(OutEvent::Pinning(
        eigenweb_pinning::client::Event::MessagePinned { message_id, .. }
    )) => {
        tracing::info!("Message {} pinned successfully", message_id);
    }

    SwarmEvent::Behaviour(OutEvent::Pinning(
        eigenweb_pinning::client::Event::MessageRetrieved { message, .. }
    )) => {
        // Process retrieved message
    }
}
```

### Server Usage (eigenweb-node)

Reference: `rust-libp2p/examples/rendezvous/src/main.rs`

```rust
use eigenweb_pinning;

#[derive(NetworkBehaviour)]
pub struct Behaviour {
    pub identify: identify::Behaviour,
    pub pinning: eigenweb_pinning::server::Behaviour<DiskStorage>,  // With custom storage
    pub ping: ping::Behaviour,
}

// In main:
let storage = DiskStorage::new("/var/lib/eigenweb/pins")?;
let pinning_config = eigenweb_pinning::Config::default()
    .with_max_message_size(1024 * 1024) // 1MB
    .with_default_ttl(86400); // 24 hours

let mut swarm = SwarmBuilder::with_new_identity()
    .with_tokio()
    .with_tcp(tcp::Config::default(), noise::Config::new, yamux::Config::default)?
    .with_behaviour(|key| Behaviour {
        identify: identify::Behaviour::new(identify::Config::new(
            "eigenweb/1.0.0".to_string(),
            key.public(),
        )),
        pinning: eigenweb_pinning::server::Behaviour::with_storage(
            storage,
            pinning_config
        ),
        ping: ping::Behaviour::new(ping::Config::default()),
    })?
    .build();

// Event loop:
while let Some(event) = swarm.next().await {
    match event {
        SwarmEvent::Behaviour(MyEvent::Pinning(
            eigenweb_pinning::server::Event::PinStored { peer, message_id }
        )) => {
            tracing::info!("Stored pin {} for peer {}", message_id, peer);
        }

        SwarmEvent::Behaviour(MyEvent::Pinning(
            eigenweb_pinning::server::Event::PinRejected { peer, reason, .. }
        )) => {
            tracing::warn!("Rejected pin from {}: {:?}", peer, reason);
        }

        _ => {}
    }
}
```

## Key Design Decisions

### 1. No Separate "eigenweb-client" Crate

Following libp2p conventions (see `rust-libp2p/protocols/`), a single crate contains both client and server implementations. Applications simply compose the appropriate behaviour.

### 2. No EventLoopHandle Abstraction

Unlike the swap CLI's custom `EventLoopHandle` pattern, canonical libp2p protocols are used directly:

```rust
// Direct access to behaviour methods
swarm.behaviour_mut().pinning.pin_message(peer_id, msg);
```

Rather than:

```rust
// Custom handle abstraction (not the libp2p way)
handle.pin_message(peer_id, msg).await?;
```

### 3. Storage via Generic Parameter

The server takes storage as a generic `S: PinStorage`, allowing:

- Default in-memory storage
- Custom disk/database storage
- Easy testing with mock storage

Reference: See how `rust-libp2p/protocols/rendezvous/src/server.rs:43-47` handles `Registrations` state.

### 4. Protocol Support Separation

- Client uses `ProtocolSupport::Outbound` only
- Server uses `ProtocolSupport::Inbound` only
- Clear separation of concerns

Reference: `rust-libp2p/protocols/request-response/src/lib.rs:57-65`

### 5. Wrapping request-response

Both client and server wrap `libp2p_request_response::Behaviour` rather than implementing the protocol from scratch. This provides:

- Automatic retry handling
- Request ID tracking
- Connection management
- Timeout handling

## State Machine Integration

For swap state machines that need to wait for pinned messages:

```rust
// In state transition:
pub async fn wait_for_pinned_message(
    swarm: &mut Swarm<Behaviour>,
    peer_id: PeerId,
    message_type: MessageType,
    timeout: Duration,
) -> Result<Message> {
    let deadline = Instant::now() + timeout;

    loop {
        tokio::select! {
            Some(event) = swarm.next() => {
                match event {
                    SwarmEvent::Behaviour(OutEvent::Pinning(
                        Event::MessageRetrieved { message, .. }
                    )) if message.matches_type(message_type) => {
                        return Ok(message);
                    }
                    _ => continue,
                }
            }
            _ = tokio::time::sleep_until(deadline) => {
                return Err(anyhow!("Timeout waiting for pinned message"));
            }
        }
    }
}
```

Alternatively, use channels to signal state machines when messages arrive.

## Testing Strategy

Reference: `rust-libp2p/protocols/rendezvous/src/rendezvous.rs:635-816`

1. **Unit tests** for storage implementations
2. **Integration tests** with mock swarms:
   ```rust
   #[tokio::test]
   async fn test_pin_and_retrieve() {
       let mut server = new_swarm(|_key| eigenweb_pinning::server::Behaviour::new(Config::default()));

       let mut client = new_swarm(|_key| eigenweb_pinning::client::Behaviour::new());

       // Test pin and retrieve flow
   }
   ```

## Configuration

```rust
pub struct Config {
    /// Maximum size of a pinned message in bytes
    pub max_message_size: usize,

    /// Default TTL if none specified (in seconds)
    pub default_ttl: u64,

    /// Maximum TTL allowed (in seconds)
    pub max_ttl: u64,

    /// Maximum number of pins per peer
    pub max_pins_per_peer: Option<usize>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_message_size: 1024 * 1024, // 1MB
            default_ttl: 86400,            // 24 hours
            max_ttl: 604800,               // 7 days
            max_pins_per_peer: Some(1000),
        }
    }
}
```

## Next Steps

1. Implement basic message types in `lib.rs`
2. Set up codec (start with CBOR via `libp2p_request_response::cbor`)
3. Implement `client::Behaviour` with basic pin/retrieve
4. Implement `server::Behaviour` with in-memory storage
5. Write integration tests
6. Add disk-based storage implementation
7. Integrate into swap CLI and eigenweb-node
