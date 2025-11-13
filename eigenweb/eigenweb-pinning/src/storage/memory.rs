//! This implementation should be used only for testing
use libp2p::PeerId;
use std::{collections::HashMap, sync::Mutex};

use crate::{signature::MessageHash, SignedPinnedMessage};

use super::Storage;

#[derive(Debug, Default)]
pub struct MemoryStorage {
    messages: Mutex<HashMap<MessageHash, SignedPinnedMessage>>,
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Storage for MemoryStorage {
    type Error = std::convert::Infallible;

    async fn pin(&self, msg: SignedPinnedMessage) -> Result<(), Self::Error> {
        let hash = msg.content_hash();
        self.messages.lock().unwrap().insert(hash, msg);
        Ok(())
    }

    async fn hashes_by_sender(&self, sender: PeerId) -> Vec<MessageHash> {
        self.messages
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, msg)| msg.message().sender == sender)
            .map(|(hash, _)| *hash)
            .collect()
    }

    async fn get_hashes_involving(
        &self,
        peer: PeerId,
    ) -> Result<(Vec<MessageHash>, Vec<MessageHash>), Self::Error> {
        let messages = self.messages.lock().unwrap();

        let incoming: Vec<MessageHash> = messages
            .iter()
            .filter(|(_, msg)| msg.message().receiver == peer)
            .map(|(hash, _)| *hash)
            .collect();

        let outgoing: Vec<MessageHash> = messages
            .iter()
            .filter(|(_, msg)| msg.message().sender == peer)
            .map(|(hash, _)| *hash)
            .collect();

        Ok((incoming, outgoing))
    }

    async fn get_by_hashes(&self, hashes: Vec<MessageHash>) -> Vec<SignedPinnedMessage> {
        let messages = self.messages.lock().unwrap();
        hashes
            .iter()
            .filter_map(|hash| messages.get(hash).cloned())
            .collect()
    }

    async fn get_by_receiver_and_hash(
        &self,
        receiver: PeerId,
        hashes: Vec<MessageHash>,
    ) -> Result<Vec<SignedPinnedMessage>, Self::Error> {
        let messages = self.messages.lock().unwrap();
        Ok(hashes
            .iter()
            .filter_map(|hash| {
                messages.get(hash).and_then(|msg| {
                    if msg.message().receiver == receiver {
                        Some(msg.clone())
                    } else {
                        None
                    }
                })
            })
            .collect())
    }

    async fn get_by_hash(
        &self,
        hash: MessageHash,
    ) -> Result<Option<SignedPinnedMessage>, Self::Error> {
        Ok(self.messages.lock().unwrap().get(&hash).cloned())
    }
}
