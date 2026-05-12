pub use monero_wallet as wallet;
pub mod wallet_rpc;

pub use ::monero_address::MoneroAddress as Address;
pub use ::monero_address::Network;
pub use ::monero_oxide_ext::{PrivateKey, PublicKey};
pub use curve25519_dalek::scalar::Scalar;
pub use swap_core::monero::primitives::*;
pub use wallet::{Daemon, Wallet, Wallets};
