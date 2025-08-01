//! Eigensync: Simple Distributed Document Synchronization
//!
//! A minimal distributed document synchronization system built on Automerge CRDTs 
//! and libp2p networking. Designed for simplicity and clarity.
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use eigensync::{hub, device, DocSync};
//! use libp2p::{identity, Swarm};
//! use uuid::Uuid;
//!
//! #[tokio::main] 
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create behaviors
//!     let hub_behaviour = hub();
//!     let device_behaviour = device();
//!     
//!     // Create document sync instances
//!     let mut hub_doc = DocSync::new();
//!     let mut device_doc = DocSync::new();
//!     
//!     // Add data and sync...
//!     hub_doc.doc_mut().put(automerge::ROOT, "key", "value")?;
//!     
//!     Ok(())
//! }
//! ```

pub mod protocol;
pub mod sync;

// Re-export the core API
pub use protocol::{hub, device, Behaviour, Request, Response, SyncEvent};
pub use sync::DocSync;

// Re-export common libp2p types for convenience
pub use libp2p::PeerId;
pub use uuid::Uuid;

/// Example showing basic document synchronization setup
/// 
/// This demonstrates creating the minimal components needed for sync.
/// See examples/device_sync.rs for a complete working example.
pub fn example_setup() -> Result<(), Box<dyn std::error::Error>> {
    use automerge::transaction::Transactable;

    // Create document sync instances
    let mut hub_doc_sync = DocSync::new();
    let mut device_doc_sync = DocSync::new();

    // Add some initial data
    hub_doc_sync.doc_mut().put(automerge::ROOT, "greeting", "Hello from hub!")?;
    device_doc_sync.doc_mut().put(automerge::ROOT, "status", "ready")?;

    println!("Created documents with initial data");
    println!("Hub has {} heads", hub_doc_sync.get_heads().len());
    println!("Device has {} heads", device_doc_sync.get_heads().len());

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use automerge::ReadDoc;

    #[test]
    fn test_api_surface() {
        // Test that we can create the basic components
        let _hub_behaviour = hub();
        let _device_behaviour = device();
        let _doc_sync = DocSync::new();
    }
    
    #[test]
    fn test_doc_sync_basic_usage() {
        use automerge::transaction::Transactable;
        
        let mut doc_sync = DocSync::new();
        doc_sync.doc_mut().put(automerge::ROOT, "test", "value").unwrap();
        
        let value = doc_sync.doc().get(automerge::ROOT, "test").unwrap().unwrap();
        assert_eq!(value.0.to_string(), "\"value\"");
    }
}
