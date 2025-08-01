//! Pure Automerge document synchronization logic
//!
//! This module provides clean document synchronization functionality
//! using Automerge's built-in sync protocol.

use automerge::{AutoCommit, sync::{self, SyncDoc, Message}};
use std::collections::HashMap;

/// Document synchronization manager
pub struct DocSync {
    /// The main Automerge document
    doc: AutoCommit,
    /// Sync states for each peer
    sync_states: HashMap<String, sync::State>,
}

impl DocSync {
    /// Create a new document sync instance
    pub fn new() -> Self {
        Self {
            doc: AutoCommit::new(),
            sync_states: HashMap::new(),
        }
    }

    /// Create from an existing Automerge document
    pub fn from_doc(doc: AutoCommit) -> Self {
        Self {
            doc,
            sync_states: HashMap::new(),
        }
    }

    /// Get a reference to the underlying document
    pub fn doc(&self) -> &AutoCommit {
        &self.doc
    }

    /// Get a mutable reference to the underlying document
    pub fn doc_mut(&mut self) -> &mut AutoCommit {
        &mut self.doc
    }

    /// Generate a sync message for a peer
    /// Returns None if no sync is needed
    pub fn generate_sync_message(&mut self, peer_id: &str) -> Option<Vec<u8>> {
        let sync_state = self.sync_states
            .entry(peer_id.to_string())
            .or_insert_with(sync::State::new);

        if let Some(message) = self.doc.sync().generate_sync_message(sync_state) {
            Some(message.encode())
        } else {
            None
        }
    }

    /// Receive and apply a sync message from a peer
    /// Returns a response message if one should be sent back
    pub fn receive_sync_message(&mut self, peer_id: &str, message: &[u8]) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>> {
        let sync_state = self.sync_states
            .entry(peer_id.to_string())
            .or_insert_with(sync::State::new);

        // Decode the message bytes into a Message
        let sync_message = Message::decode(message)?;
        self.doc.sync().receive_sync_message(sync_state, sync_message)?;
        
        // Generate response message
        if let Some(response) = self.doc.sync().generate_sync_message(sync_state) {
            Ok(Some(response.encode()))
        } else {
            Ok(None)
        }
    }

    /// Perform a complete sync with another DocSync instance
    /// This handles the full sync loop until both sides are synchronized
    pub fn sync_with(&mut self, other: &mut DocSync, _self_peer_id: &str, _other_peer_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        // Create sync states for this sync session
        let mut self_state = sync::State::new();
        let mut other_state = sync::State::new();

        loop {
            // Generate message from self to other
            let mut had_self_message = false;
            if let Some(message) = self.doc.sync().generate_sync_message(&mut self_state) {
                other.doc.sync().receive_sync_message(&mut other_state, message)?;
                had_self_message = true;
            }

            // Generate message from other to self
            let mut had_other_message = false;
            if let Some(message) = other.doc.sync().generate_sync_message(&mut other_state) {
                self.doc.sync().receive_sync_message(&mut self_state, message)?;
                had_other_message = true;
            }

            // If neither side has anything to send, we're done
            if !had_self_message && !had_other_message {
                break;
            }
        }
        Ok(())
    }

    /// Check if we need to sync with a peer
    pub fn needs_sync(&mut self, peer_id: &str) -> bool {
        let sync_state = self.sync_states
            .entry(peer_id.to_string())
            .or_insert_with(sync::State::new);

        self.doc.sync().generate_sync_message(sync_state).is_some()
    }

    /// Get the document heads (for debugging/inspection)
    pub fn get_heads(&mut self) -> Vec<automerge::ChangeHash> {
        self.doc.get_heads()
    }

    /// Get the number of operations in the document
    pub fn len(&mut self) -> usize {
        // Simple approximation - count the heads
        self.doc.get_heads().len()
    }

    /// Check if the document is empty
    pub fn is_empty(&mut self) -> bool {
        self.doc.get_heads().is_empty()
    }
}

impl Default for DocSync {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use automerge::transaction::Transactable;
    use automerge::ReadDoc;

    #[test]
    fn test_doc_sync_creation() {
        let mut doc_sync = DocSync::new();
        assert!(doc_sync.is_empty());
    }

    #[test]
    fn test_basic_sync() {
        let mut doc1 = DocSync::new();
        let mut doc2 = DocSync::new();

        // Add data to doc1
        doc1.doc_mut().put(automerge::ROOT, "key", "value1").unwrap();
        
        // Verify doc1 has the data
        let value1 = doc1.doc().get(automerge::ROOT, "key").unwrap().unwrap();
        assert_eq!(value1.0.to_string(), "\"value1\"");
        
        // Perform complete sync
        doc1.sync_with(&mut doc2, "peer1", "peer2").unwrap();
        
        // Check that doc2 now has the data
        let value2 = doc2.doc().get(automerge::ROOT, "key").unwrap().unwrap();
        assert_eq!(value2.0.to_string(), "\"value1\"");
    }

    #[test]
    fn test_bidirectional_sync() {
        let mut doc1 = DocSync::new();
        let mut doc2 = DocSync::new();

        // Add different data to each doc
        doc1.doc_mut().put(automerge::ROOT, "key1", "value1").unwrap();
        doc2.doc_mut().put(automerge::ROOT, "key2", "value2").unwrap();
        
        // Verify initial state
        assert!(doc1.doc().get(automerge::ROOT, "key1").unwrap().is_some());
        assert!(doc2.doc().get(automerge::ROOT, "key2").unwrap().is_some());
        
        // Perform complete sync
        doc1.sync_with(&mut doc2, "peer1", "peer2").unwrap();
        
        // Both docs should now have both keys
        let doc1_key1 = doc1.doc().get(automerge::ROOT, "key1").unwrap().unwrap();
        let doc1_key2 = doc1.doc().get(automerge::ROOT, "key2").unwrap().unwrap();
        let doc2_key1 = doc2.doc().get(automerge::ROOT, "key1").unwrap().unwrap();
        let doc2_key2 = doc2.doc().get(automerge::ROOT, "key2").unwrap().unwrap();
        
        assert_eq!(doc1_key1.0.to_string(), "\"value1\"");
        assert_eq!(doc1_key2.0.to_string(), "\"value2\"");
        assert_eq!(doc2_key1.0.to_string(), "\"value1\"");
        assert_eq!(doc2_key2.0.to_string(), "\"value2\"");
    }
} 