use axum::{
    body::Body,
    extract::{Request, State},
    http::{request::Parts, response, StatusCode},
    response::Response,
};
use futures::{stream::Stream, StreamExt};
use http_body_util::BodyExt;
use hyper_util::rt::TokioIo;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    time::timeout,
};
use tokio_native_tls::native_tls::TlsConnector;
use tracing::{error, info_span, Instrument};

use crate::AppState;

/// wallet2.h has a default timeout of 3 minutes + 30 seconds.
/// We assume this is a reasonable timeout. We use half of that to allow us do a single retry.
/// https://github.com/SNeedlewoods/seraphis_wallet/blob/5f714f147fd29228698070e6bd80e41ce2f86fb0/src/wallet/wallet2.h#L238
static TIMEOUT: Duration = Duration::from_secs(3 * 60 + 30).checked_div(2).unwrap();

/// Trait alias for a stream that can be used with hyper
trait HyperStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> HyperStream for T {}

#[axum::debug_handler]
pub async fn proxy_handler(State(state): State<AppState>, request: Request) -> Response {
    static POOL_SIZE: usize = 10;

    // Get the pool of nodes
    let available_pool = state
        .node_pool
        .get_top_reliable_nodes(POOL_SIZE)
        .await
        .map_err(|e| HandlerError::PoolError(e.to_string()))
        .map(|nodes| {
            let pool: Vec<(String, String, i64)> = nodes
                .into_iter()
                .map(|node| (node.scheme, node.host, node.port as i64))
                .collect();

            pool
        });

    let (request, pool) = match available_pool {
        Ok(pool) => match CloneableRequest::from_request(request).await {
            Ok(cloneable_request) => (cloneable_request, pool),
            Err(e) => {
                return Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from(e.to_string()))
                    .unwrap_or_else(|_| Response::new(Body::empty()));
            }
        },
        Err(e) => {
            // If we can't get a pool, return an error immediately
            return Response::builder()
                .status(StatusCode::SERVICE_UNAVAILABLE)
                .body(Body::from(e.to_string()))
                .unwrap_or_else(|_| Response::new(Body::empty()));
        }
    };

    let uri = request.uri().to_string();
    let method = request.jsonrpc_method();
    match proxy_to_multiple_nodes(&state, request, pool)
        .instrument(info_span!("request", uri = uri, method = method.as_deref()))
        .await
    {
        Ok(response) => response,
        Err(error) => error.to_response(),
    }
}

/// Given a Vec of nodes, proxy the given request to multiple nodes until we get a successful response
async fn proxy_to_multiple_nodes(
    state: &AppState,
    request: CloneableRequest,
    nodes: Vec<(String, String, i64)>,
) -> Result<Response, HandlerError> {
    if nodes.is_empty() {
        return Err(HandlerError::NoNodes);
    }

    let mut collected_errors: Vec<((String, String, i64), HandlerError)> = Vec::new();

    fn push_error(
        errors: &mut Vec<((String, String, i64), HandlerError)>,
        node: (String, String, i64),
        error: HandlerError,
    ) {
        tracing::debug!("Proxy request to {} failed: {}", display_node(&node), error);
        errors.push((node, error));
    }

    // Go through the nodes one by one, and proxy the request to each node
    // until we get a successful response or we run out of nodes
    // Success is defined as either:
    // - a raw HTTP response with a 200 response code
    // - a JSON-RPC response with status code 200 and no error field
    for node in nodes {
        // Node attempt logging without creating spans to reduce overhead
        let node_uri = display_node(&node);

        // Start timing the request
        let latency = std::time::Instant::now();

        let response = match proxy_to_single_node(state, request.clone(), &node)
            .instrument(info_span!(
                "connection",
                node = node_uri,
                tor = state.tor_client.is_some(),
            ))
            .await
        {
            Ok(response) => response,
            Err(e) => {
                push_error(&mut collected_errors, node, HandlerError::PhyiscalError(e));
                continue;
            }
        };

        // Calculate the latency
        let latency = latency.elapsed().as_millis() as f64;

        // Convert response to streamable to check first 1KB for errors
        let streamable_response = StreamableResponse::from_response_with_tracking(
            response,
            Some(state.node_pool.clone()),
        )
        .await
        .map_err(|e| {
            HandlerError::CloneRequestError(format!("Failed to buffer response: {}", e))
        })?;

        let error = match streamable_response.get_jsonrpc_error() {
            Some(error) => {
                // Check if we have already got two previous JSON-RPC errors
                // If we did, we assume there is a reason for it
                // We return the response as is (streaming).
                if collected_errors
                    .iter()
                    .filter(|(_, error)| matches!(error, HandlerError::JsonRpcError(_)))
                    .count()
                    >= 2
                {
                    return Ok(streamable_response.into_response());
                }

                Some(HandlerError::JsonRpcError(error))
            }
            None if streamable_response.status().is_client_error()
                || streamable_response.status().is_server_error() =>
            {
                Some(HandlerError::HttpError(streamable_response.status()))
            }
            _ => None,
        };

        match error {
            Some(error) => {
                push_error(&mut collected_errors, node, error);
            }
            None => {
                tracing::trace!(
                    "Proxy request to {} succeeded, streaming response",
                    node_uri
                );

                // Only record errors if we have gotten a successful response
                // This helps prevent logging errors if its our likely our fault (no internet)
                for (node, _) in collected_errors.iter() {
                    record_failure(&state, &node.0, &node.1, node.2).await;
                }

                // Record the success with actual latency
                record_success(&state, &node.0, &node.1, node.2, latency).await;

                // Finally return the successful streaming response
                return Ok(streamable_response.into_response());
            }
        }
    }

    Err(HandlerError::AllRequestsFailed(collected_errors))
}

/// Wraps a stream with TLS if HTTPS is being used
async fn maybe_wrap_with_tls(
    stream: impl AsyncRead + AsyncWrite + Unpin + Send + 'static,
    scheme: &str,
    host: &str,
) -> Result<Box<dyn HyperStream>, SingleRequestError> {
    if scheme == "https" {
        let tls_connector = TlsConnector::builder()
            .danger_accept_invalid_certs(true)
            .danger_accept_invalid_hostnames(true)
            .build()
            .map_err(|e| {
                SingleRequestError::ConnectionError(format!("TLS connector error: {}", e))
            })?;
        let tls_connector = tokio_native_tls::TlsConnector::from(tls_connector);

        let tls_stream = tls_connector.connect(host, stream).await.map_err(|e| {
            SingleRequestError::ConnectionError(format!("TLS connection error: {}", e))
        })?;

        Ok(Box::new(tls_stream))
    } else {
        Ok(Box::new(stream))
    }
}

/// Proxies a singular axum::Request to a single node.
/// Errors if we get a physical connection error
///
/// Important: Does NOT error if the response is a HTTP error or a JSON-RPC error
/// The caller is responsible for checking the response status and body for errors
async fn proxy_to_single_node(
    state: &crate::AppState,
    request: CloneableRequest,
    node: &(String, String, i64),
) -> Result<Response, SingleRequestError> {
    use crate::connection_pool::GuardedSender;

    if request.clearnet_whitelisted() {
        tracing::trace!("Request is whitelisted, sending over clearnet");
    }

    let use_tor = match &state.tor_client {
        Some(tc)
            if tc.bootstrap_status().ready_for_traffic() && !request.clearnet_whitelisted() =>
        {
            true
        }
        _ => false,
    };

    let key = (node.0.clone(), node.1.clone(), node.2, use_tor);

    // Try to reuse an idle HTTP connection first.
    let mut guarded_sender: Option<GuardedSender> = state.connection_pool.try_get(&key).await;

    if guarded_sender.is_none() {
        // Need to build a new TCP/Tor stream.
        let no_tls_stream: Box<dyn HyperStream> = if use_tor {
            let tor_client = state.tor_client.as_ref().ok_or_else(|| {
                SingleRequestError::ConnectionError("Tor requested but client missing".into())
            })?;
            let stream = timeout(
                TIMEOUT,
                tor_client.connect(format!("{}:{}", node.1, node.2)),
            )
            .await
            .map_err(|e| SingleRequestError::Timeout(e.to_string()))?
            .map_err(|e| SingleRequestError::ConnectionError(e.to_string()))?;

            Box::new(stream)
        } else {
            let stream = timeout(
                TIMEOUT,
                TcpStream::connect(format!("{}:{}", node.1, node.2)),
            )
            .await
            .map_err(|_| SingleRequestError::Timeout("TCP connection timed out".to_string()))?
            .map_err(|e| SingleRequestError::ConnectionError(e.to_string()))?;

            Box::new(stream)
        };

        let maybe_tls_stream = timeout(
            TIMEOUT,
            maybe_wrap_with_tls(no_tls_stream, &node.0, &node.1),
        )
        .await
        .map_err(|_| SingleRequestError::Timeout("TLS handshake timed out".to_string()))??;

        // Build an HTTP/1 connection over the stream.
        let (sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(maybe_tls_stream))
            .await
            .map_err(|e| SingleRequestError::ConnectionError(e.to_string()))?;

        // Drive the connection in the background.
        tokio::spawn(async move {
            let _ = conn.await;
        });

        // Insert into pool and obtain exclusive access for this request.
        guarded_sender = Some(
            state
                .connection_pool
                .insert_and_lock(key.clone(), sender)
                .await,
        );

        tracing::trace!(
            "Established new connection via {}{}",
            if use_tor { "Tor" } else { "clearnet" },
            if node.0 == "https" { " with TLS" } else { "" }
        );
    }

    let mut guarded_sender = guarded_sender.expect("sender must be set");

    // Forward the request to the node. URI stays relative, so no rewrite.
    let response = match guarded_sender.send_request(request.to_request()).await {
        Ok(response) => response,
        Err(e) => {
            // Connection failed, remove it from the pool
            guarded_sender.mark_failed().await;
            return Err(SingleRequestError::SendRequestError(e.to_string()));
        }
    };

    // Convert hyper Response<Incoming> to axum Response<Body>
    let (parts, body) = response.into_parts();
    let stream = body
        .into_data_stream()
        .map(|result| result.map_err(|e| axum::Error::new(e)));
    let axum_body = Body::from_stream(stream);

    Ok(Response::from_parts(parts, axum_body))
}

fn get_jsonrpc_error(body: &[u8]) -> Option<String> {
    // Try to parse as JSON
    if let Ok(json) = serde_json::from_slice::<serde_json::Value>(body) {
        // Check if there's an "error" field
        return json
            .get("error")
            .and_then(|e| e.as_str().map(|s| s.to_string()));
    }

    // If we can't parse JSON, treat it as an error
    None
}

trait RequestDifferentiator {
    /// Can this be request be proxied over clearnet?
    fn clearnet_whitelisted(&self) -> bool;
}

impl RequestDifferentiator for CloneableRequest {
    fn clearnet_whitelisted(&self) -> bool {
        match self.uri().to_string().as_str() {
            // Downloading blocks does not reveal any metadata other
            // than perhaps how far the wallet is behind or the restore
            // height.
            "/getblocks.bin" => true,
            "/gethashes.bin" => true,
            _ => false,
        }
    }
}

/// A cloneable request that buffers the body in memory
#[derive(Clone)]
pub struct CloneableRequest {
    parts: Parts,
    pub body: Vec<u8>,
}

/// A response that buffers the first 1KB for error checking and keeps the rest as a stream
pub struct StreamableResponse {
    parts: response::Parts,
    first_chunk: Vec<u8>,
    remaining_stream: Option<Pin<Box<dyn Stream<Item = Result<Vec<u8>, axum::Error>> + Send>>>,
}

/// A wrapper stream that tracks bandwidth usage
struct BandwidthTrackingStream<S> {
    inner: S,
    bandwidth_tracker: Arc<crate::pool::BandwidthTracker>,
}

impl<S> BandwidthTrackingStream<S> {
    fn new(inner: S, bandwidth_tracker: Arc<crate::pool::BandwidthTracker>) -> Self {
        Self {
            inner,
            bandwidth_tracker,
        }
    }
}

impl<S> Stream for BandwidthTrackingStream<S>
where
    S: Stream<Item = Result<Vec<u8>, axum::Error>> + Unpin,
{
    type Item = Result<Vec<u8>, axum::Error>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let result = Pin::new(&mut self.inner).poll_next(cx);

        if let std::task::Poll::Ready(Some(Ok(ref chunk))) = result {
            let chunk_size = chunk.len() as u64;
            self.bandwidth_tracker.record_bytes(chunk_size);
        }

        result
    }
}

/// A cloneable response that buffers the body in memory
#[derive(Clone)]
pub struct CloneableResponse {
    parts: response::Parts,
    body: Vec<u8>,
}

impl CloneableRequest {
    /// Convert a streaming request into a cloneable one by buffering the body
    pub async fn from_request(request: Request<Body>) -> Result<Self, axum::Error> {
        let (parts, body) = request.into_parts();
        let body_bytes = body.collect().await?.to_bytes().to_vec();

        Ok(CloneableRequest {
            parts,
            body: body_bytes,
        })
    }

    /// Convert back to a regular Request
    pub fn into_request(self) -> Request<Body> {
        Request::from_parts(self.parts, Body::from(self.body))
    }

    /// Get a new Request without consuming self
    pub fn to_request(&self) -> Request<Body> {
        Request::from_parts(self.parts.clone(), Body::from(self.body.clone()))
    }

    /// Get the URI from the request
    pub fn uri(&self) -> &axum::http::Uri {
        &self.parts.uri
    }

    /// Get the JSON-RPC method from the request body
    pub fn jsonrpc_method(&self) -> Option<String> {
        static JSON_RPC_METHOD_KEY: &str = "method";

        match serde_json::from_slice::<serde_json::Value>(&self.body) {
            Ok(json) => json
                .get(JSON_RPC_METHOD_KEY)
                .and_then(|m| m.as_str().map(|s| s.to_string())),
            Err(_) => None,
        }
    }
}

impl StreamableResponse {
    const ERROR_CHECK_SIZE: usize = 1024; // 1KB

    /// Convert a streaming response with bandwidth tracking
    pub async fn from_response_with_tracking(
        response: Response<Body>,
        node_pool: Option<Arc<crate::pool::NodePool>>,
    ) -> Result<Self, axum::Error> {
        let (parts, body) = response.into_parts();
        let mut body_stream = body.into_data_stream();

        let mut first_chunk = Vec::new();
        let mut remaining_chunks = Vec::new();
        let mut total_read = 0;

        // Collect chunks until we have at least 1KB for error checking
        while total_read < Self::ERROR_CHECK_SIZE {
            match body_stream.next().await {
                Some(Ok(chunk)) => {
                    let chunk_bytes = chunk.to_vec();
                    let needed = Self::ERROR_CHECK_SIZE - total_read;

                    if chunk_bytes.len() <= needed {
                        // Entire chunk goes to first_chunk
                        first_chunk.extend_from_slice(&chunk_bytes);
                        total_read += chunk_bytes.len();
                    } else {
                        // Split the chunk
                        first_chunk.extend_from_slice(&chunk_bytes[..needed]);
                        remaining_chunks.push(chunk_bytes[needed..].to_vec());
                        total_read += needed;
                        break;
                    }
                }
                Some(Err(e)) => return Err(e),
                None => break, // End of stream
            }
        }

        // Track bandwidth for the first chunk if we have a node pool
        if let Some(ref node_pool) = node_pool {
            node_pool.record_bandwidth(first_chunk.len() as u64);
        }

        // Create stream for remaining data
        let remaining_stream =
            if !remaining_chunks.is_empty() || total_read >= Self::ERROR_CHECK_SIZE {
                let initial_chunks = remaining_chunks.into_iter().map(Ok);
                let rest_stream = body_stream.map(|result| {
                    result
                        .map(|chunk| chunk.to_vec())
                        .map_err(|e| axum::Error::new(e))
                });
                let combined_stream = futures::stream::iter(initial_chunks).chain(rest_stream);

                // Wrap with bandwidth tracking if we have a node pool
                let final_stream: Pin<Box<dyn Stream<Item = Result<Vec<u8>, axum::Error>> + Send>> =
                    if let Some(node_pool) = node_pool.clone() {
                        let bandwidth_tracker = node_pool.get_bandwidth_tracker();
                        Box::pin(BandwidthTrackingStream::new(
                            combined_stream,
                            bandwidth_tracker,
                        ))
                    } else {
                        Box::pin(combined_stream)
                    };

                Some(final_stream)
            } else {
                None
            };

        Ok(StreamableResponse {
            parts,
            first_chunk,
            remaining_stream,
        })
    }

    /// Get the status code
    pub fn status(&self) -> StatusCode {
        self.parts.status
    }

    /// Check for JSON-RPC errors in the first chunk
    pub fn get_jsonrpc_error(&self) -> Option<String> {
        get_jsonrpc_error(&self.first_chunk)
    }

    /// Convert to a streaming response
    pub fn into_response(self) -> Response<Body> {
        let body = if let Some(remaining_stream) = self.remaining_stream {
            // Create a stream that starts with the first chunk, then continues with the rest
            let first_chunk_stream =
                futures::stream::once(futures::future::ready(Ok(self.first_chunk)));
            let combined_stream = first_chunk_stream.chain(remaining_stream);
            Body::from_stream(combined_stream)
        } else {
            // Only the first chunk exists
            Body::from(self.first_chunk)
        };

        Response::from_parts(self.parts, body)
    }

    /// Get the size of the response (first chunk only, for bandwidth tracking)
    pub fn first_chunk_size(&self) -> usize {
        self.first_chunk.len()
    }
}

impl CloneableResponse {
    /// Convert a streaming response into a cloneable one by buffering the body
    pub async fn from_response(response: Response<Body>) -> Result<Self, axum::Error> {
        let (parts, body) = response.into_parts();
        let body_bytes = body.collect().await?.to_bytes().to_vec();

        Ok(CloneableResponse {
            parts,
            body: body_bytes,
        })
    }

    /// Convert back to a regular Response
    pub fn into_response(self) -> Response<Body> {
        Response::from_parts(self.parts, Body::from(self.body))
    }

    /// Get a new Response without consuming self
    pub fn to_response(&self) -> Response<Body> {
        Response::from_parts(self.parts.clone(), Body::from(self.body.clone()))
    }

    /// Get the status code
    pub fn status(&self) -> StatusCode {
        self.parts.status
    }

    /// Check for JSON-RPC errors without consuming the response
    pub fn get_jsonrpc_error(&self) -> Option<String> {
        get_jsonrpc_error(&self.body)
    }
}

impl HandlerError {
    /// Convert HandlerError to an HTTP response
    fn to_response(&self) -> Response {
        let (status_code, error_message) = match self {
            HandlerError::NoNodes => (StatusCode::SERVICE_UNAVAILABLE, "No nodes available"),
            HandlerError::PoolError(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Pool error"),
            HandlerError::PhyiscalError(_) => (StatusCode::BAD_GATEWAY, "Connection error"),
            HandlerError::HttpError(status) => (*status, "HTTP error"),
            HandlerError::JsonRpcError(_) => (StatusCode::BAD_GATEWAY, "JSON-RPC error"),
            HandlerError::AllRequestsFailed(_) => (StatusCode::BAD_GATEWAY, "All requests failed"),
            HandlerError::CloneRequestError(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Request processing error",
            ),
        };

        let error_json = serde_json::json!({
            "error": {
                "code": status_code.as_u16(),
                "message": error_message,
                "details": self.to_string()
            }
        });

        Response::builder()
            .status(status_code)
            .header("content-type", "application/json")
            .body(Body::from(error_json.to_string()))
            .unwrap_or_else(|_| Response::new(Body::empty()))
    }
}

#[derive(Debug, Clone)]
enum HandlerError {
    NoNodes,
    PoolError(String),
    PhyiscalError(SingleRequestError),
    HttpError(axum::http::StatusCode),
    JsonRpcError(String),
    AllRequestsFailed(Vec<((String, String, i64), HandlerError)>),
    CloneRequestError(String),
}

#[derive(Debug, Clone)]
enum SingleRequestError {
    ConnectionError(String),
    SendRequestError(String),
    Timeout(String),
}

impl std::fmt::Display for HandlerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HandlerError::NoNodes => write!(f, "No nodes available"),
            HandlerError::PoolError(msg) => write!(f, "Pool error: {}", msg),
            HandlerError::PhyiscalError(msg) => write!(f, "Request error: {}", msg),
            HandlerError::JsonRpcError(msg) => write!(f, "JSON-RPC error: {}", msg),
            HandlerError::AllRequestsFailed(errors) => {
                write!(f, "All requests failed: [")?;
                for (i, (node, error)) in errors.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    let node_str = display_node(node);
                    write!(f, "{}: {}", node_str, error)?;
                }
                write!(f, "]")
            }
            HandlerError::CloneRequestError(msg) => write!(f, "Clone request error: {}", msg),
            HandlerError::HttpError(msg) => write!(f, "HTTP error: {}", msg),
        }
    }
}

impl std::fmt::Display for SingleRequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SingleRequestError::ConnectionError(msg) => write!(f, "Connection error: {}", msg),
            SingleRequestError::SendRequestError(msg) => write!(f, "Send request error: {}", msg),
            SingleRequestError::Timeout(msg) => write!(f, "Timeout: {}", msg),
        }
    }
}

fn display_node(node: &(String, String, i64)) -> String {
    format!("{}://{}:{}", node.0, node.1, node.2)
}

async fn record_success(state: &AppState, scheme: &str, host: &str, port: i64, latency_ms: f64) {
    if let Err(e) = state
        .node_pool
        .record_success(scheme, host, port, latency_ms)
        .await
    {
        error!(
            "Failed to record success for {}://{}:{}: {}",
            scheme, host, port, e
        );
    }
}

async fn record_failure(state: &AppState, scheme: &str, host: &str, port: i64) {
    if let Err(e) = state.node_pool.record_failure(scheme, host, port).await {
        error!(
            "Failed to record failure for {}://{}:{}: {}",
            scheme, host, port, e
        );
    }
}

#[axum::debug_handler]
pub async fn stats_handler(State(state): State<AppState>) -> Response {
    async move {
        match state.node_pool.get_current_status().await {
            Ok(status) => {
                let stats_json = serde_json::json!({
                    "status": "healthy",
                    "total_node_count": status.total_node_count,
                    "healthy_node_count": status.healthy_node_count,
                    "successful_health_checks": status.successful_health_checks,
                    "unsuccessful_health_checks": status.unsuccessful_health_checks,
                    "top_reliable_nodes": status.top_reliable_nodes,
                    "bandwidth_kb_per_sec": status.bandwidth_kb_per_sec
                });

                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(stats_json.to_string()))
                    .unwrap_or_else(|_| Response::new(Body::empty()))
            }
            Err(e) => {
                error!("Failed to get pool status: {}", e);
                let error_json = r#"{"status":"error","message":"Failed to get pool status"}"#;
                Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .header("content-type", "application/json")
                    .body(Body::from(error_json))
                    .unwrap_or_else(|_| Response::new(Body::empty()))
            }
        }
    }
    .instrument(info_span!("stats_request"))
    .await
}
