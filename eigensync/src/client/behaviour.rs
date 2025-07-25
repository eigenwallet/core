//! libp2p networking behaviour for eigensync client

use crate::protocol::{Request, Response};
use anyhow::Result;

/// Client-side libp2p behaviour for sending eigensync requests
pub struct ClientBehaviour {
    // TODO: Add actual libp2p request-response behaviour fields
}

impl ClientBehaviour {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn send_request(&mut self, request: Request) -> Result<Response> {
        match request {
            Request::GetChanges(_params) => {
                todo!("Implement GetChanges request")
            }
            Request::SubmitChanges(_params) => {
                todo!("Implement SubmitChanges request")
            }
        }
    }
}

impl Default for ClientBehaviour {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_behaviour_creation() {
        let _behaviour = ClientBehaviour::new();
        // Behaviour creation should not panic
    }
} 