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

/// Registrations requested per DISCOVER page. libp2p-rendezvous 0.14 cannot decode a response
/// larger than one 8 KiB read (its codec builds a fresh protobuf decoder for every read, so the
/// length prefix of a message split across reads is lost). A registration is roughly 280 bytes,
/// so 33 of them (9.3 KB, the public nodes in September 2026) never decode and the node shows
/// as `Unavailable`. Pages of 20 stay well below the limit; the cookie fetches the rest.
pub const DISCOVERY_PAGE_LIMIT: u64 = 20;

/// Most DISCOVER pages fetched from one rendezvous node in a row (200 registrations).
pub const DISCOVERY_MAX_PAGES: u32 = 10;

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
pub const SWAP_SETUP_KEEP_ALIVE: Duration = Duration::from_secs(30);
pub const SWAP_SETUP_CHANNEL_TIMEOUT: Duration = Duration::from_secs(60);
