//! Integration tests for eigensync protocol

use eigensync::protocol::*;
use eigensync::types::*;
use eigensync::*;
use serde_json::json;
use std::collections::HashMap;

/// Helper to create test actor IDs
fn create_test_actor() -> ActorId {
    ActorId(automerge::ActorId::random())
}

/// Simulate a complete handshake flow
#[test]
fn test_handshake_flow() {
    // Step 1: Client sends handshake request
    let client_actor = create_test_actor();
    let handshake_request = EigensyncRequest::Handshake(HandshakeParams {
        client_version: "1.0.0".to_string(),
        capabilities: vec!["sync".to_string(), "ack".to_string()],
        actor_id: client_actor.clone(),
    });
    
    let request_msg = EigensyncCodec::create_request(handshake_request);
    let request_id = request_msg.request_id;
    
    // Serialize and deserialize
    let encoded_request = EigensyncCodec::encode(&request_msg).unwrap();
    let decoded_request = EigensyncCodec::decode(&encoded_request).unwrap();
    assert_eq!(request_msg, decoded_request);
    
    // Step 2: Server responds with handshake result
    let handshake_response = EigensyncResponse::Handshake(HandshakeResult {
        server_version: "1.0.0".to_string(),
        protocol_version: CURRENT_VERSION,
        capabilities: vec!["sync".to_string(), "ack".to_string(), "stream".to_string()],
        session_id: "session_12345".to_string(),
        auth_required: false,
    });
    
    let response_msg = EigensyncCodec::create_response(request_id, handshake_response);
    
    // Serialize and deserialize
    let encoded_response = EigensyncCodec::encode(&response_msg).unwrap();
    let decoded_response = EigensyncCodec::decode(&encoded_response).unwrap();
    assert_eq!(response_msg, decoded_response);
    
    // Verify handshake completion
    assert_eq!(decoded_response.request_id, request_id);
    if let EigensyncPayload::Response(EigensyncResponse::Handshake(result)) = decoded_response.payload {
        assert_eq!(result.protocol_version, CURRENT_VERSION);
        assert!(!result.auth_required);
        assert!(result.capabilities.contains(&"stream".to_string()));
    } else {
        panic!("Expected handshake response");
    }
}

/// Test complete sync workflow with acknowledgments
#[test]
fn test_sync_workflow_with_acks() {
    let actor_id = create_test_actor();
    let document_id = "swap_12345";
    
    // Step 1: Client requests changes
    let get_changes_request = EigensyncRequest::GetChanges(GetChangesParams {
        document_id: document_id.to_string(),
        since_sequence: Some(0),
        since_timestamp: None,
        limit: Some(100),
        have_heads: vec![],
        stream: false,
    });
    
    let request_msg = EigensyncCodec::create_request(get_changes_request);
    let get_changes_id = request_msg.request_id;
    
    let encoded = EigensyncCodec::encode(&request_msg).unwrap();
    let decoded = EigensyncCodec::decode(&encoded).unwrap();
    assert_eq!(request_msg, decoded);
    
    // Step 2: Server responds with changes
    let changes_response = EigensyncResponse::GetChanges(GetChangesResult {
        document_id: document_id.to_string(),
        changes: vec![
            vec![1, 2, 3], // Mock change data
            vec![4, 5, 6],
        ],
        sequences: vec![1, 2],
        has_more: false,
        new_heads: vec![vec![0xAB, 0xCD]],
        total_size: 6,
    });
    
    let response_msg = EigensyncCodec::create_response(get_changes_id, changes_response);
    
    let encoded = EigensyncCodec::encode(&response_msg).unwrap();
    let decoded = EigensyncCodec::decode(&encoded).unwrap();
    assert_eq!(response_msg, decoded);
    
    // Step 3: Client submits new changes with acknowledgment request
    let submit_request = EigensyncRequest::SubmitChanges(SubmitChangesParams {
        document_id: document_id.to_string(),
        changes: vec![vec![7, 8, 9]],
        actor_id: actor_id.clone(),
        expected_sequence: Some(2),
        require_ack: true,
    });
    
    let submit_msg = EigensyncCodec::create_request(submit_request);
    let submit_id = submit_msg.request_id;
    
    let encoded = EigensyncCodec::encode(&submit_msg).unwrap();
    let decoded = EigensyncCodec::decode(&encoded).unwrap();
    assert_eq!(submit_msg, decoded);
    
    // Step 4: Server sends acknowledgment
    let ack_msg = EigensyncCodec::create_ack(
        submit_id,
        AckType::Received,
        Some(json!({"message": "Changes received and queued"})),
    );
    
    let encoded = EigensyncCodec::encode(&ack_msg).unwrap();
    let decoded = EigensyncCodec::decode(&encoded).unwrap();
    
    if let EigensyncPayload::Ack(ack) = decoded.payload {
        assert_eq!(ack.ack_request_id, submit_id);
        assert_eq!(ack.ack_type, AckType::Received);
        assert!(ack.data.is_some());
    } else {
        panic!("Expected acknowledgment");
    }
    
    // Step 5: Server processes and responds to submit request
    let submit_response = EigensyncResponse::SubmitChanges(SubmitChangesResult {
        document_id: document_id.to_string(),
        assigned_sequences: vec![3],
        new_changes_count: 1,
        new_heads: vec![vec![0xEF, 0x01]],
        conflicts: vec![],
    });
    
    let final_response = EigensyncCodec::create_response(submit_id, submit_response);
    
    let encoded = EigensyncCodec::encode(&final_response).unwrap();
    let decoded = EigensyncCodec::decode(&encoded).unwrap();
    assert_eq!(final_response, decoded);
}

/// Test error handling and recovery
#[test]
fn test_error_handling() {
    let request_id = uuid::Uuid::new_v4();
    
    // Test various error scenarios
    let error_scenarios = vec![
        (ErrorCode::NotFound, "Document not found", false),
        (ErrorCode::AuthenticationFailed, "Invalid credentials", false),
        (ErrorCode::RateLimitExceeded, "Too many requests", true),
        (ErrorCode::InternalError, "Server error", true),
        (ErrorCode::Conflict, "Sequence mismatch", false),
    ];
    
    for (code, message, should_be_retryable) in error_scenarios {
        let error_msg = EigensyncCodec::create_error_response(
            request_id,
            code,
            message.to_string(),
        );
        
        let encoded = EigensyncCodec::encode(&error_msg).unwrap();
        let decoded = EigensyncCodec::decode(&encoded).unwrap();
        assert_eq!(error_msg, decoded);
        
        if let EigensyncPayload::Response(EigensyncResponse::Error(error)) = decoded.payload {
            assert_eq!(error.code, code);
            assert_eq!(error.message, message);
            assert_eq!(error.retryable, should_be_retryable);
            
            if should_be_retryable {
                assert!(error.retry_after_ms.is_some());
            } else {
                assert!(error.retry_after_ms.is_none());
            }
        } else {
            panic!("Expected error response");
        }
    }
}

/// Test version negotiation scenarios
#[test]
fn test_version_negotiation_scenarios() {
    // Scenario 1: Compatible versions
    let client_versions = vec![1, 2, 3];
    let negotiated = EigensyncCodec::negotiate_version(&client_versions);
    assert_eq!(negotiated, Some(1));
    
    let negotiation_request = VersionNegotiationRequest {
        supported_versions: client_versions,
        capabilities: vec!["sync".to_string(), "stream".to_string()],
    };
    
    let request_msg = EigensyncMessage {
        version: CURRENT_VERSION,
        request_id: uuid::Uuid::new_v4(),
        timestamp: chrono::Utc::now(),
        payload: EigensyncPayload::VersionNegotiation(negotiation_request),
    };
    
    let encoded = EigensyncCodec::encode(&request_msg).unwrap();
    let decoded = EigensyncCodec::decode(&encoded).unwrap();
    assert_eq!(request_msg, decoded);
    
    // Scenario 2: Server responds with negotiation result
    let negotiation_response = VersionNegotiationResponse {
        selected_version: 1,
        capabilities: vec!["sync".to_string()],
        success: true,
        error: None,
    };
    
    let response_msg = EigensyncMessage {
        version: CURRENT_VERSION,
        request_id: request_msg.request_id,
        timestamp: chrono::Utc::now(),
        payload: EigensyncPayload::VersionNegotiationResponse(negotiation_response),
    };
    
    let encoded = EigensyncCodec::encode(&response_msg).unwrap();
    let decoded = EigensyncCodec::decode(&encoded).unwrap();
    
    if let EigensyncPayload::VersionNegotiationResponse(result) = decoded.payload {
        assert!(result.success);
        assert_eq!(result.selected_version, 1);
        assert!(result.error.is_none());
    } else {
        panic!("Expected version negotiation response");
    }
    
    // Scenario 3: Incompatible versions
    let incompatible_versions = vec![5, 6, 7];
    let negotiated = EigensyncCodec::negotiate_version(&incompatible_versions);
    assert_eq!(negotiated, None);
    
    let failed_response = VersionNegotiationResponse {
        selected_version: 0,
        capabilities: vec![],
        success: false,
        error: Some("No compatible version found".to_string()),
    };
    
    let failed_msg = EigensyncMessage {
        version: CURRENT_VERSION,
        request_id: uuid::Uuid::new_v4(),
        timestamp: chrono::Utc::now(),
        payload: EigensyncPayload::VersionNegotiationResponse(failed_response),
    };
    
    let encoded = EigensyncCodec::encode(&failed_msg).unwrap();
    let decoded = EigensyncCodec::decode(&encoded).unwrap();
    
    if let EigensyncPayload::VersionNegotiationResponse(result) = decoded.payload {
        assert!(!result.success);
        assert!(result.error.is_some());
    } else {
        panic!("Expected failed negotiation response");
    }
}

/// Test streaming and pagination scenarios
#[test]
fn test_streaming_and_pagination() {
    let document_id = "large_document";
    
    // Step 1: Request with streaming enabled
    let stream_request = EigensyncRequest::GetChanges(GetChangesParams {
        document_id: document_id.to_string(),
        since_sequence: Some(0),
        since_timestamp: None,
        limit: Some(10), // Small limit for pagination
        have_heads: vec![],
        stream: true,
    });
    
    let request_msg = EigensyncCodec::create_request(stream_request);
    let encoded = EigensyncCodec::encode(&request_msg).unwrap();
    let decoded = EigensyncCodec::decode(&encoded).unwrap();
    assert_eq!(request_msg, decoded);
    
    // Step 2: First batch of results
    let first_batch = EigensyncResponse::GetChanges(GetChangesResult {
        document_id: document_id.to_string(),
        changes: (0..10).map(|i| vec![i]).collect(), // 10 changes
        sequences: (1..11).collect(), // Sequences 1-10
        has_more: true, // More data available
        new_heads: vec![vec![0x10]],
        total_size: 10000, // Total size hint
    });
    
    let batch_msg = EigensyncCodec::create_response(request_msg.request_id, first_batch);
    let encoded = EigensyncCodec::encode(&batch_msg).unwrap();
    let decoded = EigensyncCodec::decode(&encoded).unwrap();
    
    if let EigensyncPayload::Response(EigensyncResponse::GetChanges(result)) = decoded.payload {
        assert_eq!(result.changes.len(), 10);
        assert!(result.has_more);
        assert_eq!(result.total_size, 10000);
    } else {
        panic!("Expected get changes response");
    }
    
    // Step 3: Request next batch
    let next_request = EigensyncRequest::GetChanges(GetChangesParams {
        document_id: document_id.to_string(),
        since_sequence: Some(10), // Continue from sequence 10
        since_timestamp: None,
        limit: Some(10),
        have_heads: vec![vec![0x10]], // Include previous heads
        stream: true,
    });
    
    let next_msg = EigensyncCodec::create_request(next_request);
    let encoded = EigensyncCodec::encode(&next_msg).unwrap();
    let decoded = EigensyncCodec::decode(&encoded).unwrap();
    assert_eq!(next_msg, decoded);
}

/// Test complex conflict resolution scenario
#[test]
fn test_conflict_resolution() {
    let document_id = "conflict_document";
    let actor1 = create_test_actor();
    let actor2 = create_test_actor();
    
    // Actor 1 submits changes
    let actor1_changes = EigensyncRequest::SubmitChanges(SubmitChangesParams {
        document_id: document_id.to_string(),
        changes: vec![vec![1, 2, 3]],
        actor_id: actor1,
        expected_sequence: Some(5),
        require_ack: true,
    });
    
    let msg1 = EigensyncCodec::create_request(actor1_changes);
    let encoded = EigensyncCodec::encode(&msg1).unwrap();
    let decoded = EigensyncCodec::decode(&encoded).unwrap();
    assert_eq!(msg1, decoded);
    
    // Actor 2 submits conflicting changes
    let actor2_changes = EigensyncRequest::SubmitChanges(SubmitChangesParams {
        document_id: document_id.to_string(),
        changes: vec![vec![4, 5, 6]],
        actor_id: actor2,
        expected_sequence: Some(5), // Same expected sequence = conflict
        require_ack: true,
    });
    
    let msg2 = EigensyncCodec::create_request(actor2_changes);
    let encoded = EigensyncCodec::encode(&msg2).unwrap();
    let decoded = EigensyncCodec::decode(&encoded).unwrap();
    assert_eq!(msg2, decoded);
    
    // Server responds with conflict information
    let conflict_response = EigensyncResponse::SubmitChanges(SubmitChangesResult {
        document_id: document_id.to_string(),
        assigned_sequences: vec![6, 7], // Both changes accepted but reordered
        new_changes_count: 2,
        new_heads: vec![vec![0xFF, 0xEE]],
        conflicts: vec![
            ConflictInfo {
                sequence: 6,
                description: "Concurrent modification at sequence 5".to_string(),
                resolution: "Applied in actor ID order".to_string(),
            },
        ],
    });
    
    let response_msg = EigensyncCodec::create_response(msg2.request_id, conflict_response);
    let encoded = EigensyncCodec::encode(&response_msg).unwrap();
    let decoded = EigensyncCodec::decode(&encoded).unwrap();
    
    if let EigensyncPayload::Response(EigensyncResponse::SubmitChanges(result)) = decoded.payload {
        assert_eq!(result.conflicts.len(), 1);
        assert_eq!(result.assigned_sequences, vec![6, 7]);
        assert_eq!(result.new_changes_count, 2);
    } else {
        panic!("Expected submit changes response with conflicts");
    }
}

/// Test ping with large payload for bandwidth testing
#[test]
fn test_ping_bandwidth_test() {
    // Create large but valid payload for bandwidth testing
    let large_payload = vec![0xAA; 1024]; // 1KB payload
    
    let ping_request = EigensyncRequest::Ping(PingParams {
        timestamp: chrono::Utc::now(),
        payload: Some(large_payload.clone()),
        echo_payload: true,
    });
    
    let request_msg = EigensyncCodec::create_request(ping_request);
    let request_time = request_msg.timestamp;
    
    let encoded = EigensyncCodec::encode(&request_msg).unwrap();
    let decoded = EigensyncCodec::decode(&encoded).unwrap();
    assert_eq!(request_msg, decoded);
    
    // Server responds with echo
    let response_time = chrono::Utc::now();
    let rtt_ms = (response_time - request_time).num_milliseconds() as f64;
    
    let ping_response = EigensyncResponse::Ping(PingResult {
        request_timestamp: request_time,
        response_timestamp: response_time,
        payload: Some(large_payload.clone()),
        rtt_ms,
    });
    
    let response_msg = EigensyncCodec::create_response(request_msg.request_id, ping_response);
    let encoded = EigensyncCodec::encode(&response_msg).unwrap();
    let decoded = EigensyncCodec::decode(&encoded).unwrap();
    
    if let EigensyncPayload::Response(EigensyncResponse::Ping(result)) = decoded.payload {
        assert_eq!(result.payload, Some(large_payload));
        assert_eq!(result.request_timestamp, request_time);
        assert!(result.rtt_ms >= 0.0);
    } else {
        panic!("Expected ping response");
    }
}

/// Test comprehensive status request with all options
#[test]
fn test_comprehensive_status() {
    let status_request = EigensyncRequest::GetStatus(GetStatusParams {
        include_stats: true,
        include_peers: true,
        include_metrics: true,
    });
    
    let request_msg = EigensyncCodec::create_request(status_request);
    let encoded = EigensyncCodec::encode(&request_msg).unwrap();
    let decoded = EigensyncCodec::decode(&encoded).unwrap();
    assert_eq!(request_msg, decoded);
    
    // Comprehensive status response
    let status_response = EigensyncResponse::GetStatus(GetStatusResult {
        server_version: "1.0.0-beta".to_string(),
        supported_versions: vec![1],
        uptime_seconds: 3600 * 24 * 7, // 1 week
        connected_peers: 15,
        document_count: 500,
        total_changes: 1_000_000,
        stats: Some(ServerStats {
            total_bytes: 1024 * 1024 * 1024, // 1GB
            bytes_sent: 1024 * 1024 * 500,   // 500MB
            bytes_received: 1024 * 1024 * 300, // 300MB
            requests_processed: 100_000,
            avg_request_time_ms: 15.5,
            memory_usage_bytes: 1024 * 1024 * 256, // 256MB
            cpu_usage_percent: 25.0,
        }),
        peers: Some(vec![
            PeerStatus {
                peer_id: "peer_001".to_string(),
                actor_id: Some("actor_001".to_string()),
                connected_at: chrono::Utc::now() - chrono::Duration::hours(2),
                last_activity: chrono::Utc::now() - chrono::Duration::minutes(5),
                document_count: 10,
                connection_quality: ConnectionQuality {
                    avg_rtt_ms: 45.0,
                    packet_loss_percent: 0.05,
                    bandwidth_bps: 1024 * 1024 * 10, // 10MB/s
                },
            },
        ]),
        metrics: Some(PerformanceMetrics {
            avg_sync_time_ms: 250.0,
            changes_per_second: 1000.0,
            cache_hit_ratio: 0.92,
            active_documents: 350,
        }),
    });
    
    let response_msg = EigensyncCodec::create_response(request_msg.request_id, status_response);
    let encoded = EigensyncCodec::encode(&response_msg).unwrap();
    let decoded = EigensyncCodec::decode(&encoded).unwrap();
    
    if let EigensyncPayload::Response(EigensyncResponse::GetStatus(result)) = decoded.payload {
        assert!(result.stats.is_some());
        assert!(result.peers.is_some());
        assert!(result.metrics.is_some());
        assert_eq!(result.connected_peers, 15);
        assert_eq!(result.document_count, 500);
        
        let stats = result.stats.unwrap();
        assert_eq!(stats.cpu_usage_percent, 25.0);
        
        let peers = result.peers.unwrap();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].peer_id, "peer_001");
        
        let metrics = result.metrics.unwrap();
        assert_eq!(metrics.cache_hit_ratio, 0.92);
    } else {
        panic!("Expected comprehensive status response");
    }
}

/// Test frame-based transport edge cases
#[test]
fn test_frame_transport_edge_cases() {
    // Test minimum frame
    let min_payload = vec![0x42];
    let frame = Frame::new(min_payload.clone()).unwrap();
    let bytes = frame.to_bytes();
    assert_eq!(bytes.len(), 8 + 1); // Header + 1 byte payload
    
    let parsed = Frame::from_bytes(&bytes).unwrap();
    assert_eq!(parsed.payload, min_payload);
    
    // Test maximum valid frame
    let max_payload = vec![0x42; MAX_FRAME_SIZE];
    let frame = Frame::new(max_payload.clone()).unwrap();
    let bytes = frame.to_bytes();
    let parsed = Frame::from_bytes(&bytes).unwrap();
    assert_eq!(parsed.payload, max_payload);
    
    // Test message splitting across frames would be handled by higher layers
    // Here we just test that large messages get proper error handling
    let oversized_request = EigensyncRequest::SubmitChanges(SubmitChangesParams {
        document_id: "huge_doc".to_string(),
        changes: vec![vec![0u8; MAX_MESSAGE_SIZE / 2]; 3], // 3 * 50% = 150% of limit
        actor_id: create_test_actor(),
        expected_sequence: None,
        require_ack: false,
    });
    
    let message = EigensyncCodec::create_request(oversized_request);
    let result = EigensyncCodec::encode(&message);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Message too large"));
} 