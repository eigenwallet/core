pub mod confirmations;
pub mod retry;
pub mod rpc;
pub mod scanner;
pub mod sweep;
pub mod verify;

pub const HARDFORK_VERSION: u8 = 16;

/// Ring size for CLSAG Bulletproofs+ transactions.
///
/// Matches the value hardcoded inside `monero-oxide` for
/// `RctType::ClsagBulletproofPlus` (see `send/mod.rs` and `send/tx.rs`), which
/// is the protocol-required ring length for the current hard fork. There is
/// no exported constant upstream, so we mirror the magic number here.
pub const RING_LEN: u8 = 16;
