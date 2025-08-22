pub mod wallet;
pub mod wallet_rpc;

pub use ::monero::network::Network;
pub use ::monero::{Address, PrivateKey, PublicKey};
pub use curve25519_dalek::scalar::Scalar;
pub use wallet::{Daemon, Wallet, Wallets, WatchRequest};
pub use swap_core::monero::primitives::*;