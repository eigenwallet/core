use anyhow::Context;
use bdk_electrum::electrum_client::HeaderNotification;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::convert::{TryFrom, TryInto};
use std::fmt;
use std::ops::Add;
use typeshare::typeshare;

/// Represent a block height, or block number, expressed in absolute block
/// count.
///
/// E.g. The transaction was included in block #655123, 655123 blocks
/// after the genesis block.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BlockHeight(u32);

impl From<BlockHeight> for u32 {
    fn from(height: BlockHeight) -> Self {
        height.0
    }
}

impl From<u32> for BlockHeight {
    fn from(height: u32) -> Self {
        Self(height)
    }
}

impl TryFrom<HeaderNotification> for BlockHeight {
    type Error = anyhow::Error;

    fn try_from(value: HeaderNotification) -> Result<Self, Self::Error> {
        Ok(Self(
            value
                .height
                .try_into()
                .context("Failed to fit usize into u32")?,
        ))
    }
}

impl Add<u32> for BlockHeight {
    type Output = BlockHeight;
    fn add(self, rhs: u32) -> Self::Output {
        BlockHeight(self.0 + rhs)
    }
}

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
