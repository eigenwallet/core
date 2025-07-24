//! libp2p networking behaviour for eigensync server

use crate::protocol::{EigensyncRequest, EigensyncResponse};
use anyhow::Result;

/// Server-side libp2p behaviour for handling eigensync requests
pub struct ServerBehaviour {
    // TODO: Add actual libp2p request-response behaviour fields
}

impl ServerBehaviour {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn handle_request(&mut self, request: EigensyncRequest) -> Result<EigensyncResponse> {
        match request {
            EigensyncRequest::GetChanges(_params) => {
                todo!("Implement GetChanges handler")
            }
            EigensyncRequest::SubmitChanges(_params) => {
                todo!("Implement SubmitChanges handler")
            }
            EigensyncRequest::Ping(_params) => {
                todo!("Implement Ping handler")
            }
            EigensyncRequest::GetStatus(_params) => {
                todo!("Implement GetStatus handler")
            }
            EigensyncRequest::Handshake(_params) => {
                todo!("Implement Handshake handler")
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