use libp2p::PeerId;
use std::collections::HashMap;

use crate::{SignedPinnedMessage, signature::MessageHash};

pub trait Storage: Send {
    type Error: std::error::Error + Send + Sync + 'static;

    fn store(&mut self, msg: SignedPinnedMessage) -> Result<(), Self::Error>;
    fn hashes_by_sender(&self, sender: PeerId) -> Vec<MessageHash>;
    fn hashes_by_receiver(&self, receiver: PeerId) -> Vec<MessageHash>;
    fn get_by_hashes(&self, hashes: Vec<MessageHash>) -> Vec<SignedPinnedMessage>;
    fn get_by_receiver_and_hash(
        &self,
        receiver: PeerId,
        hashes: Vec<MessageHash>,
    ) -> Vec<SignedPinnedMessage>;
}

#[derive(Debug, Default)]
pub struct MemoryStorage {
    messages: HashMap<MessageHash, SignedPinnedMessage>,
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

    fn hashes_by_sender(&self, sender: PeerId) -> Vec<MessageHash> {
        self.messages
            .iter()
            .filter(|(_, msg)| msg.message().sender == sender)
            .map(|(hash, _)| *hash)
            .collect()
    }

    fn hashes_by_receiver(&self, receiver: PeerId) -> Vec<MessageHash> {
        self.messages
            .iter()
            .filter(|(_, msg)| msg.message().receiver == receiver)
            .map(|(hash, _)| *hash)
            .collect()
    }

    fn get_by_hashes(&self, hashes: Vec<MessageHash>) -> Vec<SignedPinnedMessage> {
        hashes
            .iter()
            .filter_map(|hash| self.messages.get(hash).cloned())
            .collect()
    }

    fn get_by_receiver_and_hash(
        &self,
        receiver: PeerId,
        hashes: Vec<MessageHash>,
    ) -> Vec<SignedPinnedMessage> {
        hashes
            .iter()
            .filter_map(|hash| {
                self.messages.get(hash).and_then(|msg| {
                    if msg.message().receiver == receiver {
                        Some(msg.clone())
                    } else {
                        None
                    }
                })
            })
            .collect()
    }
}
