//! Protocol definitions for eigensync communication
//!
//! This module defines the wire protocol used for communication between
//! eigensync clients and servers. The protocol is versioned and uses
//! serde_cbor for serialization with length-prefixed frames.

use crate::types::{ActorId, Result, Error};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Protocol version for version negotiation
pub const CURRENT_VERSION: u32 = 1;

/// Minimum supported protocol version
pub const MIN_SUPPORTED_VERSION: u32 = 1;

/// Maximum supported protocol version  
pub const MAX_SUPPORTED_VERSION: u32 = 1;

/// Maximum message size to prevent DoS attacks (10 MB)
pub const MAX_MESSAGE_SIZE: usize = 10 * 1024 * 1024;

/// Maximum frame size for streaming (1 MB)
pub const MAX_FRAME_SIZE: usize = 1024 * 1024;

/// Default request timeout
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Frame header size (4 bytes for length)
pub const FRAME_HEADER_SIZE: usize = 4;

/// Protocol magic bytes for frame validation
pub const PROTOCOL_MAGIC: [u8; 4] = [0xE1, 0x6E, 0x53, 0x79]; // "EiSy"

/// Main message envelope for all eigensync communications
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EigensyncMessage {
    /// Protocol version
    pub version: u32,
    /// Unique request identifier for matching responses
    pub request_id: uuid::Uuid,
    /// Message timestamp for ordering and timeout detection
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Message payload
    pub payload: EigensyncPayload,
}

/// Union type for all message payloads
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data")]
pub enum EigensyncPayload {
    Request(EigensyncRequest),
    Response(EigensyncResponse),
    /// Version negotiation message
    VersionNegotiation(VersionNegotiationRequest),
    /// Version negotiation response
    VersionNegotiationResponse(VersionNegotiationResponse),
    /// Acknowledgment message
    Ack(AckMessage),
}

/// All possible request types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "params")]
pub enum EigensyncRequest {
    /// Get changes from server since a given point
    GetChanges(GetChangesParams),
    /// Submit new changes to server
    SubmitChanges(SubmitChangesParams),
    /// Ping for connectivity testing
    Ping(PingParams),
    /// Get server status/info
    GetStatus(GetStatusParams),
    /// Handshake request for connection establishment
    Handshake(HandshakeParams),
}

/// All possible response types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "result")]
pub enum EigensyncResponse {
    /// Response to GetChanges request
    GetChanges(GetChangesResult),
    /// Response to SubmitChanges request
    SubmitChanges(SubmitChangesResult),
    /// Response to Ping request
    Ping(PingResult),
    /// Response to GetStatus request
    GetStatus(GetStatusResult),
    /// Response to Handshake request
    Handshake(HandshakeResult),
    /// Error response for any request
    Error(ErrorResult),
}

// Version negotiation messages

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VersionNegotiationRequest {
    /// Client supported versions (in preference order)
    pub supported_versions: Vec<u32>,
    /// Client capabilities
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VersionNegotiationResponse {
    /// Selected protocol version
    pub selected_version: u32,
    /// Server capabilities
    pub capabilities: Vec<String>,
    /// Whether negotiation was successful
    pub success: bool,
    /// Error message if negotiation failed
    pub error: Option<String>,
}

// Acknowledgment message

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AckMessage {
    /// ID of message being acknowledged
    pub ack_request_id: uuid::Uuid,
    /// Acknowledgment type
    pub ack_type: AckType,
    /// Optional payload
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AckType {
    /// Simple acknowledgment
    Received,
    /// Processing started
    Processing,
    /// Processing completed successfully
    Completed,
    /// Processing failed
    Failed,
}

// Request parameters (enhanced)

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GetChangesParams {
    /// Document to get changes for (typically swap_id)
    pub document_id: String,
    /// Only return changes after this sequence number
    pub since_sequence: Option<u64>,
    /// Only return changes after this timestamp
    pub since_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    /// Maximum number of changes to return (for pagination)
    pub limit: Option<u32>,
    /// Automerge heads we already have (to optimize sync)
    pub have_heads: Vec<Vec<u8>>, // Serialized ChangeHash
    /// Request streaming if available
    pub stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SubmitChangesParams {
    /// Document these changes apply to
    pub document_id: String,
    /// Serialized Automerge changes
    pub changes: Vec<Vec<u8>>,
    /// Actor ID that created these changes
    pub actor_id: ActorId,
    /// Expected sequence number for optimistic concurrency control
    pub expected_sequence: Option<u64>,
    /// Request acknowledgment
    pub require_ack: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PingParams {
    /// Timestamp when ping was sent
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Optional payload for bandwidth testing
    pub payload: Option<Vec<u8>>,
    /// Expected echo behavior
    pub echo_payload: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GetStatusParams {
    /// Include detailed statistics
    pub include_stats: bool,
    /// Include information about other peers
    pub include_peers: bool,
    /// Include performance metrics
    pub include_metrics: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HandshakeParams {
    /// Client version
    pub client_version: String,
    /// Client capabilities
    pub capabilities: Vec<String>,
    /// Actor ID for this client
    pub actor_id: ActorId,
}

// Response results (enhanced)

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GetChangesResult {
    /// Document ID these changes apply to
    pub document_id: String,
    /// Serialized Automerge changes
    pub changes: Vec<Vec<u8>>,
    /// Sequence numbers for each change
    pub sequences: Vec<u64>,
    /// Whether there are more changes available
    pub has_more: bool,
    /// Current document heads after applying these changes
    pub new_heads: Vec<Vec<u8>>, // Serialized ChangeHash
    /// Total size of all changes in bytes
    pub total_size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SubmitChangesResult {
    /// Document ID changes were applied to
    pub document_id: String,
    /// Sequence numbers assigned to the submitted changes
    pub assigned_sequences: Vec<u64>,
    /// Number of changes that were actually new (not duplicates)
    pub new_changes_count: u32,
    /// Current document heads after applying changes
    pub new_heads: Vec<Vec<u8>>, // Serialized ChangeHash
    /// Conflicts detected during application
    pub conflicts: Vec<ConflictInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConflictInfo {
    /// Sequence number where conflict occurred
    pub sequence: u64,
    /// Description of the conflict
    pub description: String,
    /// How the conflict was resolved
    pub resolution: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PingResult {
    /// Timestamp from the request
    pub request_timestamp: chrono::DateTime<chrono::Utc>,
    /// Timestamp when server processed the request
    pub response_timestamp: chrono::DateTime<chrono::Utc>,
    /// Echo back any payload that was sent
    pub payload: Option<Vec<u8>>,
    /// Round-trip time in milliseconds
    pub rtt_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GetStatusResult {
    /// Server version/build info
    pub server_version: String,
    /// Protocol versions supported
    pub supported_versions: Vec<u32>,
    /// Server uptime in seconds
    pub uptime_seconds: u64,
    /// Number of connected peers
    pub connected_peers: u32,
    /// Number of documents being tracked
    pub document_count: u64,
    /// Total number of changes stored
    pub total_changes: u64,
    /// Optional detailed statistics
    pub stats: Option<ServerStats>,
    /// Optional peer information
    pub peers: Option<Vec<PeerStatus>>,
    /// Optional performance metrics
    pub metrics: Option<PerformanceMetrics>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HandshakeResult {
    /// Server version
    pub server_version: String,
    /// Agreed protocol version
    pub protocol_version: u32,
    /// Server capabilities
    pub capabilities: Vec<String>,
    /// Session ID for this connection
    pub session_id: String,
    /// Authentication required
    pub auth_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ErrorResult {
    /// Error code for programmatic handling
    pub code: ErrorCode,
    /// Human-readable error message
    pub message: String,
    /// Optional additional details
    pub details: Option<serde_json::Value>,
    /// Whether the error is retryable
    pub retryable: bool,
    /// Suggested retry delay in milliseconds
    pub retry_after_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerStats {
    /// Total bytes stored
    pub total_bytes: u64,
    /// Bytes sent since startup
    pub bytes_sent: u64,
    /// Bytes received since startup
    pub bytes_received: u64,
    /// Number of requests processed
    pub requests_processed: u64,
    /// Average request processing time in milliseconds
    pub avg_request_time_ms: f64,
    /// Memory usage in bytes
    pub memory_usage_bytes: u64,
    /// CPU usage percentage
    pub cpu_usage_percent: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PeerStatus {
    /// Peer ID
    pub peer_id: String,
    /// Associated actor ID if authenticated
    pub actor_id: Option<String>,
    /// When peer connected
    pub connected_at: chrono::DateTime<chrono::Utc>,
    /// Last activity timestamp
    pub last_activity: chrono::DateTime<chrono::Utc>,
    /// Number of documents this peer is syncing
    pub document_count: u32,
    /// Connection quality metrics
    pub connection_quality: ConnectionQuality,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConnectionQuality {
    /// Average round-trip time in milliseconds
    pub avg_rtt_ms: f64,
    /// Packet loss percentage
    pub packet_loss_percent: f64,
    /// Bandwidth utilization in bytes/second
    pub bandwidth_bps: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PerformanceMetrics {
    /// Average sync time in milliseconds
    pub avg_sync_time_ms: f64,
    /// Changes processed per second
    pub changes_per_second: f64,
    /// Cache hit ratio
    pub cache_hit_ratio: f64,
    /// Active document count
    pub active_documents: u64,
}

/// Error codes for programmatic error handling
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u32)]
pub enum ErrorCode {
    /// Unknown or internal server error
    InternalError = 1000,
    /// Invalid request format or parameters
    InvalidRequest = 1001,
    /// Authentication failed
    AuthenticationFailed = 1002,
    /// Requested resource not found
    NotFound = 1003,
    /// Rate limit exceeded
    RateLimitExceeded = 1004,
    /// Storage quota exceeded
    QuotaExceeded = 1005,
    /// Version not supported
    UnsupportedVersion = 1006,
    /// Request timeout
    Timeout = 1007,
    /// Conflict in optimistic concurrency control
    Conflict = 1008,
    /// Invalid actor/peer mapping
    InvalidActorMapping = 1009,
    /// Document not found
    DocumentNotFound = 1010,
    /// Invalid sequence number
    InvalidSequence = 1011,
    /// Malformed message
    MalformedMessage = 1012,
    /// Frame too large
    FrameTooLarge = 1013,
    /// Protocol version mismatch
    VersionMismatch = 1014,
}

impl ErrorCode {
    /// Convert error code to human-readable string
    pub fn as_str(&self) -> &'static str {
        match self {
            ErrorCode::InternalError => "internal_error",
            ErrorCode::InvalidRequest => "invalid_request",
            ErrorCode::AuthenticationFailed => "authentication_failed",
            ErrorCode::NotFound => "not_found",
            ErrorCode::RateLimitExceeded => "rate_limit_exceeded",
            ErrorCode::QuotaExceeded => "quota_exceeded",
            ErrorCode::UnsupportedVersion => "unsupported_version",
            ErrorCode::Timeout => "timeout",
            ErrorCode::Conflict => "conflict",
            ErrorCode::InvalidActorMapping => "invalid_actor_mapping",
            ErrorCode::DocumentNotFound => "document_not_found",
            ErrorCode::InvalidSequence => "invalid_sequence",
            ErrorCode::MalformedMessage => "malformed_message",
            ErrorCode::FrameTooLarge => "frame_too_large",
            ErrorCode::VersionMismatch => "version_mismatch",
        }
    }

    /// Whether this error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            ErrorCode::InternalError
                | ErrorCode::RateLimitExceeded
                | ErrorCode::QuotaExceeded
                | ErrorCode::Timeout
        )
    }
}

impl From<ErrorCode> for Error {
    fn from(code: ErrorCode) -> Self {
        Error::Protocol {
            message: format!("Protocol error: {}", code.as_str()),
        }
    }
}

/// Frame for stream-based communication
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    /// Frame magic bytes for validation
    pub magic: [u8; 4],
    /// Frame payload length
    pub length: u32,
    /// Frame payload
    pub payload: Vec<u8>,
}

impl Frame {
    /// Create a new frame with payload
    pub fn new(payload: Vec<u8>) -> Result<Self> {
        if payload.len() > MAX_FRAME_SIZE {
            return Err(Error::Protocol {
                message: format!(
                    "Frame payload too large: {} bytes > {} max",
                    payload.len(),
                    MAX_FRAME_SIZE
                ),
            });
        }

        Ok(Frame {
            magic: PROTOCOL_MAGIC,
            length: payload.len() as u32,
            payload,
        })
    }

    /// Serialize frame to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(8 + self.payload.len());
        bytes.extend_from_slice(&self.magic);
        bytes.extend_from_slice(&self.length.to_be_bytes());
        bytes.extend_from_slice(&self.payload);
        bytes
    }

    /// Parse frame from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 8 {
            return Err(Error::Protocol {
                message: "Frame too short for header".to_string(),
            });
        }

        let magic = [bytes[0], bytes[1], bytes[2], bytes[3]];
        if magic != PROTOCOL_MAGIC {
            return Err(Error::Protocol {
                message: format!("Invalid frame magic: {:?}", magic),
            });
        }

        let length = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        
        if length as usize > MAX_FRAME_SIZE {
            return Err(Error::Protocol {
                message: format!("Frame too large: {} bytes", length),
            });
        }

        if bytes.len() < 8 + length as usize {
            return Err(Error::Protocol {
                message: "Frame incomplete".to_string(),
            });
        }

        let payload = bytes[8..8 + length as usize].to_vec();

        Ok(Frame {
            magic,
            length,
            payload,
        })
    }
}

/// Codec for serializing/deserializing eigensync messages
pub struct EigensyncCodec;

impl EigensyncCodec {
    /// Serialize message to bytes with frame header
    pub fn encode(message: &EigensyncMessage) -> Result<Vec<u8>> {
        let payload = serde_cbor::to_vec(message)?;
        
        if payload.len() > MAX_MESSAGE_SIZE {
            return Err(Error::Protocol {
                message: format!(
                    "Message too large: {} bytes > {} max",
                    payload.len(),
                    MAX_MESSAGE_SIZE
                ),
            });
        }

        let frame = Frame::new(payload)?;
        Ok(frame.to_bytes())
    }

    /// Deserialize message from framed bytes
    pub fn decode(data: &[u8]) -> Result<EigensyncMessage> {
        let frame = Frame::from_bytes(data)?;
        
        let message: EigensyncMessage = serde_cbor::from_slice(&frame.payload)?;
        
        // Validate version
        if !Self::is_version_supported(message.version) {
            return Err(Error::Protocol {
                message: format!(
                    "Unsupported protocol version: {} (supported: {}-{})",
                    message.version,
                    MIN_SUPPORTED_VERSION,
                    MAX_SUPPORTED_VERSION
                ),
            });
        }

        Ok(message)
    }

    /// Create a request message
    pub fn create_request(request: EigensyncRequest) -> EigensyncMessage {
        EigensyncMessage {
            version: CURRENT_VERSION,
            request_id: uuid::Uuid::new_v4(),
            timestamp: chrono::Utc::now(),
            payload: EigensyncPayload::Request(request),
        }
    }

    /// Create a response message
    pub fn create_response(
        request_id: uuid::Uuid,
        response: EigensyncResponse,
    ) -> EigensyncMessage {
        EigensyncMessage {
            version: CURRENT_VERSION,
            request_id,
            timestamp: chrono::Utc::now(),
            payload: EigensyncPayload::Response(response),
        }
    }

    /// Create an error response
    pub fn create_error_response(
        request_id: uuid::Uuid,
        code: ErrorCode,
        message: String,
    ) -> EigensyncMessage {
        Self::create_response(
            request_id,
            EigensyncResponse::Error(ErrorResult {
                code,
                message,
                details: None,
                retryable: code.is_retryable(),
                retry_after_ms: if code.is_retryable() { Some(1000) } else { None },
            }),
        )
    }

    /// Create acknowledgment message
    pub fn create_ack(
        request_id: uuid::Uuid,
        ack_type: AckType,
        data: Option<serde_json::Value>,
    ) -> EigensyncMessage {
        EigensyncMessage {
            version: CURRENT_VERSION,
            request_id: uuid::Uuid::new_v4(),
            timestamp: chrono::Utc::now(),
            payload: EigensyncPayload::Ack(AckMessage {
                ack_request_id: request_id,
                ack_type,
                data,
            }),
        }
    }

    /// Check if protocol version is supported
    pub fn is_version_supported(version: u32) -> bool {
        version >= MIN_SUPPORTED_VERSION && version <= MAX_SUPPORTED_VERSION
    }

    /// Get supported version range
    pub fn get_supported_versions() -> Vec<u32> {
        (MIN_SUPPORTED_VERSION..=MAX_SUPPORTED_VERSION).collect()
    }

    /// Negotiate protocol version
    pub fn negotiate_version(client_versions: &[u32]) -> Option<u32> {
        let supported = Self::get_supported_versions();
        
        // Find highest mutually supported version
        for &version in client_versions.iter() {
            if supported.contains(&version) {
                return Some(version);
            }
        }
        
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create_test_actor() -> ActorId {
        ActorId(automerge::ActorId::random())
    }

    #[test]
    fn test_protocol_constants() {
        assert_eq!(CURRENT_VERSION, 1);
        assert_eq!(MIN_SUPPORTED_VERSION, 1);
        assert_eq!(MAX_SUPPORTED_VERSION, 1);
        assert_eq!(PROTOCOL_MAGIC, [0xE1, 0x6E, 0x53, 0x79]);
    }

    #[test]
    fn test_frame_creation_and_serialization() {
        let payload = b"test payload".to_vec();
        let frame = Frame::new(payload.clone()).unwrap();
        
        assert_eq!(frame.magic, PROTOCOL_MAGIC);
        assert_eq!(frame.length, payload.len() as u32);
        assert_eq!(frame.payload, payload);
        
        let bytes = frame.to_bytes();
        assert_eq!(bytes.len(), 8 + payload.len());
        
        // Check magic bytes
        assert_eq!(&bytes[0..4], &PROTOCOL_MAGIC);
        
        // Check length
        let length = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        assert_eq!(length, payload.len() as u32);
        
        // Check payload
        assert_eq!(&bytes[8..], &payload);
    }

    #[test]
    fn test_frame_parsing() {
        let payload = b"hello world".to_vec();
        let frame = Frame::new(payload.clone()).unwrap();
        let bytes = frame.to_bytes();
        
        let parsed_frame = Frame::from_bytes(&bytes).unwrap();
        assert_eq!(parsed_frame, frame);
    }

    #[test]
    fn test_frame_invalid_magic() {
        let mut bytes = vec![0xFF, 0xFF, 0xFF, 0xFF]; // Invalid magic
        bytes.extend_from_slice(&5u32.to_be_bytes()); // Length
        bytes.extend_from_slice(b"hello"); // Payload
        
        let result = Frame::from_bytes(&bytes);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid frame magic"));
    }

    #[test]
    fn test_frame_too_short() {
        let bytes = vec![0xE1, 0x6E, 0x53]; // Only 3 bytes
        let result = Frame::from_bytes(&bytes);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Frame too short"));
    }

    #[test]
    fn test_frame_incomplete() {
        let mut bytes = PROTOCOL_MAGIC.to_vec();
        bytes.extend_from_slice(&10u32.to_be_bytes()); // Claims 10 bytes
        bytes.extend_from_slice(b"short"); // Only 5 bytes
        
        let result = Frame::from_bytes(&bytes);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Frame incomplete"));
    }

    #[test]
    fn test_frame_too_large() {
        let payload = vec![0u8; MAX_FRAME_SIZE + 1];
        let result = Frame::new(payload);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Frame payload too large"));
    }

    #[test]
    fn test_codec_ping_roundtrip() {
        let request = EigensyncRequest::Ping(PingParams {
            timestamp: chrono::Utc::now(),
            payload: Some(b"test".to_vec()),
            echo_payload: true,
        });
        
        let message = EigensyncCodec::create_request(request);
        let encoded = EigensyncCodec::encode(&message).unwrap();
        let decoded = EigensyncCodec::decode(&encoded).unwrap();
        
        assert_eq!(message.version, decoded.version);
        assert_eq!(message.request_id, decoded.request_id);
        assert_eq!(message.payload, decoded.payload);
        
        // Verify it's a ping request
        if let EigensyncPayload::Request(EigensyncRequest::Ping(params)) = decoded.payload {
            assert!(params.echo_payload);
            assert_eq!(params.payload, Some(b"test".to_vec()));
        } else {
            panic!("Expected ping request");
        }
    }

    #[test]
    fn test_codec_get_changes_roundtrip() {
        let request = EigensyncRequest::GetChanges(GetChangesParams {
            document_id: "test-doc".to_string(),
            since_sequence: Some(42),
            since_timestamp: Some(chrono::Utc::now()),
            limit: Some(100),
            have_heads: vec![vec![1, 2, 3], vec![4, 5, 6]],
            stream: true,
        });
        
        let message = EigensyncCodec::create_request(request);
        let encoded = EigensyncCodec::encode(&message).unwrap();
        let decoded = EigensyncCodec::decode(&encoded).unwrap();
        
        assert_eq!(message, decoded);
    }

    #[test]
    fn test_codec_submit_changes_roundtrip() {
        let request = EigensyncRequest::SubmitChanges(SubmitChangesParams {
            document_id: "test-doc".to_string(),
            changes: vec![vec![1, 2, 3], vec![4, 5, 6]],
            actor_id: create_test_actor(),
            expected_sequence: Some(10),
            require_ack: true,
        });
        
        let message = EigensyncCodec::create_request(request);
        let encoded = EigensyncCodec::encode(&message).unwrap();
        let decoded = EigensyncCodec::decode(&encoded).unwrap();
        
        assert_eq!(message, decoded);
    }

    #[test]
    fn test_codec_handshake_roundtrip() {
        let request = EigensyncRequest::Handshake(HandshakeParams {
            client_version: "1.0.0".to_string(),
            capabilities: vec!["sync".to_string(), "stream".to_string()],
            actor_id: create_test_actor(),
        });
        
        let message = EigensyncCodec::create_request(request);
        let encoded = EigensyncCodec::encode(&message).unwrap();
        let decoded = EigensyncCodec::decode(&encoded).unwrap();
        
        assert_eq!(message, decoded);
    }

    #[test]
    fn test_codec_error_response_roundtrip() {
        let request_id = uuid::Uuid::new_v4();
        let message = EigensyncCodec::create_error_response(
            request_id,
            ErrorCode::NotFound,
            "Resource not found".to_string(),
        );
        
        let encoded = EigensyncCodec::encode(&message).unwrap();
        let decoded = EigensyncCodec::decode(&encoded).unwrap();
        
        assert_eq!(message, decoded);
        
        // Verify error details
        if let EigensyncPayload::Response(EigensyncResponse::Error(error)) = decoded.payload {
            assert_eq!(error.code, ErrorCode::NotFound);
            assert_eq!(error.message, "Resource not found");
            assert!(!error.retryable);
            assert_eq!(error.retry_after_ms, None);
        } else {
            panic!("Expected error response");
        }
    }

    #[test]
    fn test_codec_ack_message_roundtrip() {
        let original_request_id = uuid::Uuid::new_v4();
        let ack_data = Some(json!({"status": "processing", "progress": 0.5}));
        
        let message = EigensyncCodec::create_ack(
            original_request_id,
            AckType::Processing,
            ack_data.clone(),
        );
        
        let encoded = EigensyncCodec::encode(&message).unwrap();
        let decoded = EigensyncCodec::decode(&encoded).unwrap();
        
        if let EigensyncPayload::Ack(ack) = decoded.payload {
            assert_eq!(ack.ack_request_id, original_request_id);
            assert_eq!(ack.ack_type, AckType::Processing);
            assert_eq!(ack.data, ack_data);
        } else {
            panic!("Expected ack message");
        }
    }

    #[test]
    fn test_codec_version_negotiation_roundtrip() {
        let request = VersionNegotiationRequest {
            supported_versions: vec![1, 2, 3],
            capabilities: vec!["sync".to_string(), "stream".to_string()],
        };
        
        let message = EigensyncMessage {
            version: CURRENT_VERSION,
            request_id: uuid::Uuid::new_v4(),
            timestamp: chrono::Utc::now(),
            payload: EigensyncPayload::VersionNegotiation(request),
        };
        
        let encoded = EigensyncCodec::encode(&message).unwrap();
        let decoded = EigensyncCodec::decode(&encoded).unwrap();
        
        assert_eq!(message, decoded);
    }

    #[test]
    fn test_complex_response_roundtrip() {
        let response = EigensyncResponse::GetChanges(GetChangesResult {
            document_id: "complex-doc".to_string(),
            changes: vec![
                vec![1, 2, 3, 4, 5],
                vec![6, 7, 8, 9, 10],
                vec![11, 12, 13, 14, 15],
            ],
            sequences: vec![100, 101, 102],
            has_more: true,
            new_heads: vec![vec![0xFF, 0xFE], vec![0xFD, 0xFC]],
            total_size: 1024,
        });
        
        let message = EigensyncCodec::create_response(uuid::Uuid::new_v4(), response);
        let encoded = EigensyncCodec::encode(&message).unwrap();
        let decoded = EigensyncCodec::decode(&encoded).unwrap();
        
        assert_eq!(message, decoded);
    }

    #[test]
    fn test_message_size_limit() {
        // Create a message that would exceed the size limit
        let large_payload = vec![0u8; MAX_MESSAGE_SIZE / 2]; // Each change is half the limit
        let request = EigensyncRequest::SubmitChanges(SubmitChangesParams {
            document_id: "test".to_string(),
            changes: vec![large_payload.clone(), large_payload], // Total exceeds limit
            actor_id: create_test_actor(),
            expected_sequence: None,
            require_ack: false,
        });
        
        let message = EigensyncCodec::create_request(request);
        let result = EigensyncCodec::encode(&message);
        
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Message too large"));
    }

    #[test]
    fn test_version_support_check() {
        assert!(EigensyncCodec::is_version_supported(1));
        assert!(!EigensyncCodec::is_version_supported(0));
        assert!(!EigensyncCodec::is_version_supported(2));
        assert!(!EigensyncCodec::is_version_supported(999));
    }

    #[test]
    fn test_version_negotiation() {
        // Client supports versions 1, 2, 3
        let client_versions = vec![1, 2, 3];
        let negotiated = EigensyncCodec::negotiate_version(&client_versions);
        assert_eq!(negotiated, Some(1)); // Should pick the first supported version

        // Client doesn't support any server versions
        let client_versions = vec![5, 6, 7];
        let negotiated = EigensyncCodec::negotiate_version(&client_versions);
        assert_eq!(negotiated, None);

        // Empty client versions
        let negotiated = EigensyncCodec::negotiate_version(&[]);
        assert_eq!(negotiated, None);
    }

    #[test]
    fn test_unsupported_version_decode() {
        let mut message = EigensyncCodec::create_request(EigensyncRequest::Ping(PingParams {
            timestamp: chrono::Utc::now(),
            payload: None,
            echo_payload: false,
        }));
        
        // Set unsupported version
        message.version = 999;
        
        let payload = serde_cbor::to_vec(&message).unwrap();
        let frame = Frame::new(payload).unwrap();
        let encoded = frame.to_bytes();
        
        let result = EigensyncCodec::decode(&encoded);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unsupported protocol version"));
    }

    #[test]
    fn test_malformed_cbor_decode() {
        let malformed_payload = vec![0xFF, 0xFE, 0xFD, 0xFC]; // Invalid CBOR
        let frame = Frame::new(malformed_payload).unwrap();
        let encoded = frame.to_bytes();
        
        let result = EigensyncCodec::decode(&encoded);
        assert!(result.is_err());
        // Should be a CBOR deserialization error
    }

    #[test]
    fn test_error_code_properties() {
        // Test retryable errors
        assert!(ErrorCode::InternalError.is_retryable());
        assert!(ErrorCode::RateLimitExceeded.is_retryable());
        assert!(ErrorCode::QuotaExceeded.is_retryable());
        assert!(ErrorCode::Timeout.is_retryable());
        
        // Test non-retryable errors
        assert!(!ErrorCode::InvalidRequest.is_retryable());
        assert!(!ErrorCode::AuthenticationFailed.is_retryable());
        assert!(!ErrorCode::NotFound.is_retryable());
        assert!(!ErrorCode::UnsupportedVersion.is_retryable());
        
        // Test error string conversion
        assert_eq!(ErrorCode::InternalError.as_str(), "internal_error");
        assert_eq!(ErrorCode::DocumentNotFound.as_str(), "document_not_found");
        assert_eq!(ErrorCode::VersionMismatch.as_str(), "version_mismatch");
    }

    #[test]
    fn test_comprehensive_status_response() {
        let status_result = GetStatusResult {
            server_version: "1.0.0".to_string(),
            supported_versions: vec![1, 2],
            uptime_seconds: 86400,
            connected_peers: 5,
            document_count: 100,
            total_changes: 50000,
            stats: Some(ServerStats {
                total_bytes: 1024 * 1024 * 100, // 100 MB
                bytes_sent: 1024 * 1024 * 50,   // 50 MB
                bytes_received: 1024 * 1024 * 30, // 30 MB
                requests_processed: 10000,
                avg_request_time_ms: 25.5,
                memory_usage_bytes: 1024 * 1024 * 200, // 200 MB
                cpu_usage_percent: 15.7,
            }),
            peers: Some(vec![
                PeerStatus {
                    peer_id: "peer1".to_string(),
                    actor_id: Some("actor1".to_string()),
                    connected_at: chrono::Utc::now(),
                    last_activity: chrono::Utc::now(),
                    document_count: 5,
                    connection_quality: ConnectionQuality {
                        avg_rtt_ms: 50.0,
                        packet_loss_percent: 0.1,
                        bandwidth_bps: 1024 * 1024, // 1 MB/s
                    },
                },
            ]),
            metrics: Some(PerformanceMetrics {
                avg_sync_time_ms: 100.0,
                changes_per_second: 500.0,
                cache_hit_ratio: 0.85,
                active_documents: 50,
            }),
        };
        
        let response = EigensyncResponse::GetStatus(status_result);
        let message = EigensyncCodec::create_response(uuid::Uuid::new_v4(), response);
        
        let encoded = EigensyncCodec::encode(&message).unwrap();
        let decoded = EigensyncCodec::decode(&encoded).unwrap();
        
        assert_eq!(message, decoded);
    }

    #[test]
    fn test_submit_changes_with_conflicts() {
        let conflicts = vec![
            ConflictInfo {
                sequence: 100,
                description: "Concurrent modification".to_string(),
                resolution: "Last-writer-wins".to_string(),
            },
            ConflictInfo {
                sequence: 101,
                description: "Duplicate key".to_string(), 
                resolution: "Merged values".to_string(),
            },
        ];
        
        let result = SubmitChangesResult {
            document_id: "conflict-doc".to_string(),
            assigned_sequences: vec![100, 101, 102],
            new_changes_count: 2,
            new_heads: vec![vec![0x01, 0x02], vec![0x03, 0x04]],
            conflicts,
        };
        
        let response = EigensyncResponse::SubmitChanges(result);
        let message = EigensyncCodec::create_response(uuid::Uuid::new_v4(), response);
        
        let encoded = EigensyncCodec::encode(&message).unwrap();
        let decoded = EigensyncCodec::decode(&encoded).unwrap();
        
        assert_eq!(message, decoded);
        
        // Verify conflicts are preserved
        if let EigensyncPayload::Response(EigensyncResponse::SubmitChanges(result)) = decoded.payload {
            assert_eq!(result.conflicts.len(), 2);
            assert_eq!(result.conflicts[0].sequence, 100);
            assert_eq!(result.conflicts[1].description, "Duplicate key");
        } else {
            panic!("Expected submit changes response");
        }
    }

    #[test] 
    fn test_all_ack_types() {
        let request_id = uuid::Uuid::new_v4();
        
        let ack_types = vec![
            AckType::Received,
            AckType::Processing,
            AckType::Completed,
            AckType::Failed,
        ];
        
        for ack_type in ack_types {
            let message = EigensyncCodec::create_ack(request_id, ack_type.clone(), None);
            
            let encoded = EigensyncCodec::encode(&message).unwrap();
            let decoded = EigensyncCodec::decode(&encoded).unwrap();
            
            if let EigensyncPayload::Ack(ack) = decoded.payload {
                assert_eq!(ack.ack_type, ack_type);
                assert_eq!(ack.ack_request_id, request_id);
            } else {
                panic!("Expected ack message");
            }
        }
    }

    #[test]
    fn test_empty_collections() {
        // Test with empty vectors and None values
        let request = EigensyncRequest::GetChanges(GetChangesParams {
            document_id: "empty-doc".to_string(),
            since_sequence: None,
            since_timestamp: None,
            limit: None,
            have_heads: vec![], // Empty
            stream: false,
        });
        
        let message = EigensyncCodec::create_request(request);
        let encoded = EigensyncCodec::encode(&message).unwrap();
        let decoded = EigensyncCodec::decode(&encoded).unwrap();
        
        assert_eq!(message, decoded);
    }

    #[test]
    fn test_large_but_valid_message() {
        // Create a message that's large but within frame size limits
        // We need to account for the message envelope overhead, so use a smaller payload
        let large_but_valid = vec![0u8; MAX_FRAME_SIZE / 8]; // 128KB - well within frame limits
        
        let request = EigensyncRequest::SubmitChanges(SubmitChangesParams {
            document_id: "large-doc".to_string(),
            changes: vec![large_but_valid],
            actor_id: create_test_actor(),
            expected_sequence: None,
            require_ack: false,
        });
        
        let message = EigensyncCodec::create_request(request);
        let encoded = EigensyncCodec::encode(&message).unwrap();
        let decoded = EigensyncCodec::decode(&encoded).unwrap();
        
        assert_eq!(message, decoded);
    }

    #[test]
    fn test_timestamp_preservation() {
        let fixed_timestamp = chrono::DateTime::parse_from_rfc3339("2024-01-01T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        
        let mut message = EigensyncCodec::create_request(EigensyncRequest::Ping(PingParams {
            timestamp: fixed_timestamp,
            payload: None,
            echo_payload: false,
        }));
        
        // Override with fixed timestamp
        message.timestamp = fixed_timestamp;
        
        let encoded = EigensyncCodec::encode(&message).unwrap();
        let decoded = EigensyncCodec::decode(&encoded).unwrap();
        
        assert_eq!(decoded.timestamp, fixed_timestamp);
    }
} 