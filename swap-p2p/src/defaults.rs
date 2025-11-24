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
pub const CACHED_QUOTE_EXPIRY: Duration = Duration::from_secs(120);
pub const QUOTE_INTERVAL: Duration = Duration::from_secs(5);
pub const QUOTE_REDIAL_INTERVAL: Duration = Duration::from_secs(1);
pub const QUOTE_REDIAL_MAX_INTERVAL: Duration = Duration::from_secs(30);
pub const QUOTE_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

// Swap setup
pub const NEGOTIATION_TIMEOUT: Duration = Duration::from_secs(120);
pub const SWAP_SETUP_KEEP_ALIVE: Duration = Duration::from_secs(30);
pub const SWAP_SETUP_CHANNEL_TIMEOUT: Duration = Duration::from_secs(60);
