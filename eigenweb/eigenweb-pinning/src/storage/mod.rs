use libp2p::PeerId;
use std::future::Future;

use crate::{signature::MessageHash, SignedPinnedMessage};

#[cfg(feature = "memory")]
mod memory;

#[cfg(feature = "memory")]
pub use memory::MemoryStorage;

pub trait Storage: Send {
    type Error: std::error::Error + Send + Sync + 'static;

    fn pin(&self, msg: SignedPinnedMessage)
        -> impl Future<Output = Result<(), Self::Error>> + Send;
    fn hashes_by_sender(&self, sender: PeerId) -> impl Future<Output = Vec<MessageHash>> + Send;
    fn get_hashes_involving(
        &self,
        peer: PeerId,
    ) -> impl Future<Output = Result<(Vec<MessageHash>, Vec<MessageHash>), Self::Error>> + Send;
    fn get_by_hashes(
        &self,
        hashes: Vec<MessageHash>,
    ) -> impl Future<Output = Vec<SignedPinnedMessage>> + Send;
    fn get_by_hash(
        &self,
        hash: MessageHash,
    ) -> impl Future<Output = Result<Option<SignedPinnedMessage>, Self::Error>> + Send;
    fn get_by_receiver_and_hash(
        &self,
        receiver: PeerId,
        hashes: Vec<MessageHash>,
    ) -> impl Future<Output = Result<Vec<SignedPinnedMessage>, Self::Error>> + Send;
}
