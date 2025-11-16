use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;
use std::ops::Add;
use typeshare::typeshare;

pub use bitcoin_wallet::BlockHeight;

/// Represent a timelock, expressed in relative block height as defined in
/// [BIP68](https://github.com/bitcoin/bips/blob/master/bip-0068.mediawiki).
/// E.g. The timelock expires 10 blocks after the reference transaction is
/// mined.
#[derive(Debug, Copy, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(transparent)]
#[typeshare]
pub struct CancelTimelock(pub u32);

impl From<CancelTimelock> for u32 {
    fn from(cancel_timelock: CancelTimelock) -> Self {
        cancel_timelock.0
    }
}

impl From<u32> for CancelTimelock {
    fn from(number_of_blocks: u32) -> Self {
        Self(number_of_blocks)
    }
}

impl CancelTimelock {
    pub const fn new(number_of_blocks: u32) -> Self {
        Self(number_of_blocks)
    }
}

impl Add<CancelTimelock> for BlockHeight {
    type Output = BlockHeight;

    fn add(self, rhs: CancelTimelock) -> Self::Output {
        self + rhs.0
    }
}

impl PartialOrd<CancelTimelock> for u32 {
    fn partial_cmp(&self, other: &CancelTimelock) -> Option<Ordering> {
        self.partial_cmp(&other.0)
    }
}

impl PartialEq<CancelTimelock> for u32 {
    fn eq(&self, other: &CancelTimelock) -> bool {
        self.eq(&other.0)
    }
}

impl fmt::Display for CancelTimelock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} blocks", self.0)
    }
}

/// Represent a timelock, expressed in relative block height as defined in
/// [BIP68](https://github.com/bitcoin/bips/blob/master/bip-0068.mediawiki).
/// E.g. The timelock expires 10 blocks after the reference transaction is
/// mined.
#[derive(Debug, Copy, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(transparent)]
#[typeshare]
pub struct PunishTimelock(pub u32);

impl From<PunishTimelock> for u32 {
    fn from(punish_timelock: PunishTimelock) -> Self {
        punish_timelock.0
    }
}

impl From<u32> for PunishTimelock {
    fn from(number_of_blocks: u32) -> Self {
        Self(number_of_blocks)
    }
}

impl PunishTimelock {
    pub const fn new(number_of_blocks: u32) -> Self {
        Self(number_of_blocks)
    }
}

impl Add<PunishTimelock> for BlockHeight {
    type Output = BlockHeight;

    fn add(self, rhs: PunishTimelock) -> Self::Output {
        self + rhs.0
    }
}

impl PartialOrd<PunishTimelock> for u32 {
    fn partial_cmp(&self, other: &PunishTimelock) -> Option<Ordering> {
        self.partial_cmp(&other.0)
    }
}

impl PartialEq<PunishTimelock> for u32 {
    fn eq(&self, other: &PunishTimelock) -> bool {
        self.eq(&other.0)
    }
}

#[typeshare]
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(tag = "type", content = "content")]
pub enum ExpiredTimelocks {
    None { blocks_left: u32 },
    Cancel { blocks_left: u32 },
    Punish,
}

impl ExpiredTimelocks {
    /// Check whether the timelock on the cancel transaction has expired.
    ///
    /// Retuns `true` even if the swap has already been canceled or punished.
    pub fn cancel_timelock_expired(&self) -> bool {
        !matches!(self, ExpiredTimelocks::None { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin_wallet::*;
    use crate::bitcoin::*;
    use bitcoin::secp256k1;
    use ecdsa_fun::fun::marker::{NonZero, Public};
    use ecdsa_fun::fun::Point;
    use rand::rngs::OsRng;

    #[test]
    fn lock_confirmations_le_to_cancel_timelock_no_timelock_expired() {
        let tx_lock_status = ScriptStatus::from_confirmations(4);
        let tx_cancel_status = ScriptStatus::Unseen;

        let expired_timelock = current_epoch(
            CancelTimelock::new(5),
            PunishTimelock::new(5),
            tx_lock_status,
            tx_cancel_status,
        );

        assert!(matches!(expired_timelock, ExpiredTimelocks::None { .. }));
    }

    #[test]
    fn lock_confirmations_ge_to_cancel_timelock_cancel_timelock_expired() {
        let tx_lock_status = ScriptStatus::from_confirmations(5);
        let tx_cancel_status = ScriptStatus::Unseen;

        let expired_timelock = current_epoch(
            CancelTimelock::new(5),
            PunishTimelock::new(5),
            tx_lock_status,
            tx_cancel_status,
        );

        assert!(matches!(expired_timelock, ExpiredTimelocks::Cancel { .. }));
    }

    #[test]
    fn cancel_confirmations_ge_to_punish_timelock_punish_timelock_expired() {
        let tx_lock_status = ScriptStatus::from_confirmations(10);
        let tx_cancel_status = ScriptStatus::from_confirmations(5);

        let expired_timelock = current_epoch(
            CancelTimelock::new(5),
            PunishTimelock::new(5),
            tx_lock_status,
            tx_cancel_status,
        );

        assert_eq!(expired_timelock, ExpiredTimelocks::Punish)
    }

    #[test]
    fn tx_early_refund_has_correct_weight() {
        // TxEarlyRefund should have the same weight as other similar transactions
        assert_eq!(TxEarlyRefund::weight(), 548);

        // It should be the same as TxRedeem and TxRefund weights since they have similar structure
        assert_eq!(TxEarlyRefund::weight() as u64, TxRedeem::weight().to_wu());
        assert_eq!(TxEarlyRefund::weight() as u64, TxRefund::weight().to_wu());
    }

    #[test]
    fn compare_point_hex() {
        // secp256kfun Point and secp256k1 PublicKey should have the same bytes and hex representation
        let secp = secp256k1::Secp256k1::default();
        let keypair = secp256k1::Keypair::new(&secp, &mut OsRng);

        let pubkey = keypair.public_key();
        let point: Point<_, Public, NonZero> = Point::from_bytes(pubkey.serialize()).unwrap();

        assert_eq!(pubkey.to_string(), point.to_string());
    }
}
