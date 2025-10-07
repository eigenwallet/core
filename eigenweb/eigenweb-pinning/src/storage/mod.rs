use libp2p::PeerId;
use std::collections::HashMap;

use crate::SignedPinnedMessage;

pub trait Storage: Send {
    type Error: std::error::Error + Send + Sync + 'static;

    fn store(&mut self, msg: SignedPinnedMessage) -> Result<(), Self::Error>;
    fn retrieve(&self, receiver: PeerId) -> Vec<SignedPinnedMessage>;
    fn hashes_by_sender(&self, sender: PeerId) -> Vec<[u8; 32]>;
    fn get_by_hash(&self, hash: [u8; 32]) -> Option<SignedPinnedMessage>;
}

#[derive(Debug, Default)]
pub struct MemoryStorage {
    messages: HashMap<[u8; 32], SignedPinnedMessage>,
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Storage for MemoryStorage {
    type Error = std::convert::Infallible;

    fn store(&mut self, msg: SignedPinnedMessage) -> Result<(), Self::Error> {
        let hash = msg.content_hash();
        self.messages.insert(hash, msg);
        Ok(())
    }

    fn retrieve(&self, receiver: PeerId) -> Vec<SignedPinnedMessage> {
        self.messages
            .values()
            .filter(|msg| msg.message().receiver == receiver)
            .cloned()
            .collect()
    }

    fn hashes_by_sender(&self, sender: PeerId) -> Vec<[u8; 32]> {
        self.messages
            .iter()
            .filter(|(_, msg)| msg.message().sender == sender)
            .map(|(hash, _)| *hash)
            .collect()
    }

    fn get_by_hash(&self, hash: [u8; 32]) -> Option<SignedPinnedMessage> {
        self.messages.get(&hash).cloned()
    }
}
