//! Protocol definitions for eigensync communication

use crate::types::{ActorId, DocumentId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "params")]
pub enum Request {
    /// Get changes from server since a given point
    GetChanges(GetChangesParams),
    /// Submit new changes to server
    SubmitChanges(SubmitChangesParams),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "result")]
pub enum Response {
    /// Response to GetChanges request
    GetChanges(GetChangesResult),
    /// Response to SubmitChanges request
    SubmitChanges(SubmitChangesResult),
    /// Error response for any request
    Error(ErrorResult),
}

// Request parameters

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GetChangesParams {
    /// Document to get changes for
    pub document_id: DocumentId,
    /// Only return changes after this sequence number
    pub since_sequence: Option<u64>,
    /// Maximum number of changes to return (for pagination)
    pub limit: Option<u32>,
    /// Automerge heads we already have (to optimize sync)
    pub have_heads: Vec<Vec<u8>>, // Serialized ChangeHash
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SubmitChangesParams {
    /// Document these changes apply to
    pub document_id: DocumentId,
    /// Serialized Automerge changes
    pub changes: Vec<Vec<u8>>,
    /// Actor ID that created these changes
    pub actor_id: ActorId,
    /// Expected sequence number for optimistic concurrency control
    pub expected_sequence: Option<u64>,
}

// Response results

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GetChangesResult {
    /// Document ID these changes apply to
    pub document_id: DocumentId,
    /// Serialized Automerge changes
    pub changes: Vec<Vec<u8>>,
    /// Sequence numbers for each change
    pub sequences: Vec<u64>,
    /// Whether there are more changes available
    pub has_more: bool,
    /// Current document heads after applying these changes
    pub new_heads: Vec<Vec<u8>>, // Serialized ChangeHash
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SubmitChangesResult {
    /// Document ID changes were applied to
    pub document_id: DocumentId,
    /// Sequence numbers assigned to the submitted changes
    pub assigned_sequences: Vec<u64>,
    /// Number of changes that were actually new (not duplicates)
    pub new_changes_count: u32,
    /// Current document heads after applying changes
    pub new_heads: Vec<Vec<u8>>, // Serialized ChangeHash
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ErrorResult {
    /// Error code for programmatic handling
    pub code: ErrorCode,
    /// Human-readable error message
    pub message: String,
    /// Whether the error is retryable
    pub retryable: bool,
}

/// Error codes for programmatic error handling
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u32)]
pub enum ErrorCode {
    /// Unknown or internal server error
    InternalError = 1000,
    /// Invalid request format or parameters
    InvalidRequest = 1001,
    /// Requested resource not found
    NotFound = 1003,
    /// Conflict in optimistic concurrency control
    Conflict = 1008,
    /// Document not found
    DocumentNotFound = 1010,
    /// Invalid sequence number
    InvalidSequence = 1011,
}

impl ErrorCode {
    /// Whether this error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(self, ErrorCode::InternalError)
    }
} 