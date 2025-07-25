//! libp2p networking behaviour for eigensync server

use crate::protocol::{Request, Response};
use anyhow::Result;

/// Server-side libp2p behaviour for handling eigensync requests
pub struct ServerBehaviour {
    // TODO: Add actual libp2p request-response behaviour fields
}

impl ServerBehaviour {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn handle_request(&mut self, request: Request) -> Result<Response> {
        match request {
            Request::GetChanges(_params) => {
                todo!("Implement GetChanges handler")
            }
            Request::SubmitChanges(_params) => {
                todo!("Implement SubmitChanges handler")
            }
        }
    }
}

impl Default for ServerBehaviour {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_behaviour_creation() {
        let _behaviour = ServerBehaviour::new();
        // Behaviour creation should not panic
    }
} 