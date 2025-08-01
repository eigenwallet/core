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
pub use protocol::{client, server, Behaviour, Request, Response};
pub use sync::DocSync;

// Re-export common libp2p types for convenience
pub use libp2p::PeerId;
pub use uuid::Uuid;
