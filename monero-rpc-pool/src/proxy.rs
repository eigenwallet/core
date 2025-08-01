use axum::{
    body::Body,
    extract::{Request, State},
    http::{request::Parts, response, StatusCode},
    response::Response,
};
use http_body_util::BodyExt;
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_native_tls::native_tls::TlsConnector;
use tracing::{error, info_span, Instrument};

use crate::AppState;

/// Trait alias for a stream that can be used with hyper
trait HyperStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> HyperStream for T {}

#[axum::debug_handler]
pub async fn proxy_handler(State(state): State<AppState>, request: Request) -> Response {
    static POOL_SIZE: usize = 10;

    // Get the pool of nodes
    let available_pool = {
        let nodes = state
            .node_pool
            .get_top_reliable_nodes(POOL_SIZE)
            .await
            .map_err(|e| HandlerError::PoolError(e.to_string()))
            .unwrap();

        let pool: Vec<(String, String, i64)> = nodes
            .into_iter()
            .map(|node| (node.scheme, node.host, node.port as i64))
            .collect();

        pool
    };

    let request = CloneableRequest::from_request(request).await.unwrap();

    // Record request bandwidth (upload)
    state.node_pool.record_bandwidth(request.body.len() as u64);

    let uri = request.uri().to_string();
    let method = request.jsonrpc_method();
    match proxy_to_multiple_nodes(&state, request, available_pool)
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

        let response = match proxy_to_single_node(request.clone(), &node, state.tor_client.clone())
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

        // Convert response to cloneable to avoid consumption issues
        let cloneable_response = CloneableResponse::from_response(response)
            .await
            .map_err(|e| {
                HandlerError::CloneRequestError(format!("Failed to buffer response: {}", e))
            })?;

        let error = match cloneable_response.get_jsonrpc_error() {
            Some(error) => {
                // Check if we have already got two previous JSON-RPC errors
                // If we did, we assume there is a reason for it
                // We return the response as is.
                if collected_errors
                    .iter()
                    .filter(|(_, error)| matches!(error, HandlerError::JsonRpcError(_)))
                    .count()
                    >= 2
                {
                    return Ok(cloneable_response.into_response());
                }

                Some(HandlerError::JsonRpcError(error))
            }
            None if cloneable_response.status().is_client_error()
                || cloneable_response.status().is_server_error() =>
            {
                Some(HandlerError::HttpError(cloneable_response.status()))
            }
            _ => None,
        };

        match error {
            Some(error) => {
                tracing::info!("Proxy request to {} failed: {}", node_uri, error);
                push_error(&mut collected_errors, node, error);
            }
            None => {
                let response_size_bytes = cloneable_response.body.len() as u64;
                tracing::info!(
                    "Proxy request to {} succeeded with size {}kb",
                    node_uri,
                    (response_size_bytes as f64 / 1024.0)
                );

                // Record bandwidth usage
                state.node_pool.record_bandwidth(response_size_bytes);

                // Only record errors if we have gotten a successful response
                // This helps prevent logging errors if its our likely our fault (no internet)
                for (node, _) in collected_errors.iter() {
                    record_failure(&state, &node.0, &node.1, node.2).await;
                }

                // Record the success with actual latency
                record_success(&state, &node.0, &node.1, node.2, latency).await;

                // Finally return the successful response
                return Ok(cloneable_response.into_response());
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
        let tls_connector = TlsConnector::builder().build().map_err(|e| {
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
    request: CloneableRequest,
    node: &(String, String, i64),
    tor_client: Option<crate::TorClientArc>,
) -> Result<Response, SingleRequestError> {
    if request.clearnet_whitelisted() {
        tracing::info!("Request is whitelisted, sending over clearnet");
    }

    let response = match tor_client {
        // If Tor client is ready for traffic, use it
        Some(tor_client)
            if tor_client.bootstrap_status().ready_for_traffic()
                // If the request is whitelisted, we don't want to use Tor
                && !request.clearnet_whitelisted() =>
        {
            let stream = tor_client
                .connect(format!("{}:{}", node.1, node.2))
                .await
                .map_err(|e| SingleRequestError::ConnectionError(e.to_string()))?;

            // Wrap with TLS if using HTTPS
            let stream = maybe_wrap_with_tls(stream, &node.0, &node.1).await?;

            let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
                .await
                .map_err(|e| SingleRequestError::ConnectionError(e.to_string()))?;

            tracing::info!(
                "Connected to node via Tor{}",
                if node.0 == "https" { " with TLS" } else { "" }
            );

            tokio::task::spawn(async move {
                if let Err(err) = conn.await {
                    println!("Connection failed: {:?}", err);
                }
            });

            // Forward the request to the node
            // No need to rewrite the URI because the request.uri() is relative
            sender
                .send_request(request.to_request())
                .await
                .map_err(|e| SingleRequestError::SendRequestError(e.to_string()))?
        }
        // Otherwise send over clearnet
        _ => {
            let stream = TcpStream::connect(format!("{}:{}", node.1, node.2))
                .await
                .map_err(|e| SingleRequestError::ConnectionError(e.to_string()))?;

            // Wrap with TLS if using HTTPS
            let stream = maybe_wrap_with_tls(stream, &node.0, &node.1).await?;

            let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
                .await
                .map_err(|e| SingleRequestError::ConnectionError(e.to_string()))?;

            tracing::info!(
                "Connected to node via clearnet{}",
                if node.0 == "https" { " with TLS" } else { "" }
            );

            tokio::task::spawn(async move {
                if let Err(err) = conn.await {
                    println!("Connection failed: {:?}", err);
                }
            });

            sender
                .send_request(request.to_request())
                .await
                .map_err(|e| SingleRequestError::SendRequestError(e.to_string()))?
        }
    };

    // Convert hyper Response<Incoming> to axum Response<Body>
    let (parts, body) = response.into_parts();
    let body_bytes = body
        .collect()
        .await
        .map_err(|e| SingleRequestError::CollectResponseError(e.to_string()))?
        .to_bytes();
    let axum_body = Body::from(body_bytes);

    let response = Response::from_parts(parts, axum_body);

    Ok(response)
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
    CollectResponseError(String),
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
            SingleRequestError::CollectResponseError(msg) => {
                write!(f, "Collect response error: {}", msg)
            }
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
