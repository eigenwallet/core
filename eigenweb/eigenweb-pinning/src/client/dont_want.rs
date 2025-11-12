use std::{collections::{HashMap, HashSet}, hash::Hash, sync::Arc};

use libp2p::PeerId;

pub struct DontWantSet<H: Hash + Eq + Clone>(HashMap<PeerId, Arc<HashSet<H>>>);

impl<H: Hash + Eq + Clone> DontWantSet<H> {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    /// Call this whenever we definitely know a server has a message or we definitely know he does not want it 
    pub fn mark_does_not_want(&mut self, peer_id: PeerId, hash: H) {
        let mut set = self.dont_want_set_or_default(peer_id).as_ref().clone();
        set.insert(hash);
        self.0.insert(peer_id, Arc::new(set));
    }

    /// Call this whenever we definitively know a server does not have a message
    pub fn mark_does_not_have(&mut self, peer_id: PeerId, hash: H) {
        if !self.has_set(&peer_id) {
            // If we don't have a set for the peer yet, we cannot remove anything from it
            // We also don't want to wrongly insert an empty set for the peer as this as this may be
            // interpreted as "we are sure the server has no messages"
            return;
        }

        let mut set = self.dont_want_set_or_default(peer_id).as_ref().clone();
        set.remove(&hash);
        self.0.insert(peer_id, Arc::new(set));
    }

    pub fn replace(&mut self, peer_id: PeerId, hashes: impl IntoIterator<Item = H>) {
        self.0.insert(peer_id, Arc::new(hashes.into_iter().collect()));
    }

    pub fn dont_want_read_only(&self, peer_id: &PeerId) -> Option<Arc<HashSet<H>>> {
        self.0.get(peer_id).cloned()
    }

    pub fn has_set(&self, peer_id: &PeerId) -> bool {
        self.0.contains_key(peer_id)
    }

    fn dont_want_set_or_default(&mut self, peer_id: PeerId) -> &mut Arc<HashSet<H>> {
        self.0.entry(peer_id).or_insert_with(|| Arc::new(HashSet::new()))
    }
}