use std::time::Duration;

pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
pub const IDLE_CONNECTION_TIMEOUT: Duration = Duration::from_secs(15 * 60); // 15 minutes

pub const BACKOFF_MULTIPLIER: f64 = 1.5;

// Redial
pub const REDIAL_INITIAL_INTERVAL: Duration = Duration::from_secs(1);
pub const REDIAL_MAX_INTERVAL: Duration = Duration::from_secs(10);

// Rendezvous
pub const RENDEZVOUS_REDIAL_MAX_INTERVAL: Duration = Duration::from_secs(60);

// Rendezvous discovery
pub const DISCOVERY_INITIAL_INTERVAL: Duration = Duration::from_secs(1);
pub const DISCOVERY_MAX_INTERVAL: Duration = Duration::from_secs(60 * 3);
pub const DISCOVERY_INTERVAL: Duration = Duration::from_secs(60);

// Rendezvous register
pub const RENDEZVOUS_RETRY_INITIAL_INTERVAL: Duration = Duration::from_secs(1);
pub const RENDEZVOUS_RETRY_MAX_INTERVAL: Duration = Duration::from_secs(60);

// Quote
pub const CACHED_QUOTE_EXPIRY: Duration = Duration::from_secs(180);
pub const QUOTE_INTERVAL: Duration = Duration::from_secs(45);
pub const QUOTE_REDIAL_INTERVAL: Duration = Duration::from_secs(1);
pub const QUOTE_REDIAL_MAX_INTERVAL: Duration = Duration::from_secs(30);
pub const QUOTE_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

// Swap setup
pub const NEGOTIATION_TIMEOUT: Duration = Duration::from_secs(120);
/// How long Alice keeps the setup open while Bob's user decides whether to lock the Bitcoin.
/// Must be longer than the approval timeout of Bob.
pub const SWAP_SETUP_DECISION_TIMEOUT: Duration = Duration::from_secs(180);
/// How long Bob waits for Alice to confirm that she persisted the swap.
pub const SWAP_SETUP_PERSISTED_TIMEOUT: Duration = Duration::from_secs(60);
/// How long Alice may take to persist a swap Bob committed to. Committing can take
/// another few seconds on a busy database, so this stays well below
/// [`SWAP_SETUP_PERSISTED_TIMEOUT`] and her confirmation still reaches Bob.
pub const SWAP_SETUP_PERSIST_TIMEOUT: Duration = Duration::from_secs(20);
const _: () =
    assert!(SWAP_SETUP_PERSIST_TIMEOUT.as_secs() * 2 < SWAP_SETUP_PERSISTED_TIMEOUT.as_secs());
pub const SWAP_SETUP_KEEP_ALIVE: Duration = Duration::from_secs(30);
pub const SWAP_SETUP_CHANNEL_TIMEOUT: Duration = Duration::from_secs(60);
