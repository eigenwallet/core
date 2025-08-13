use axum::{
    body::Body,
    extract::{Request, State},
    http::{request::Parts, response, StatusCode},
    response::Response,
};
use http_body_util::BodyExt;
use hyper_util::rt::TokioIo;
use std::pin::Pin;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    time::timeout,
};

use tracing::{error, info_span, Instrument};

use crate::AppState;

/// wallet2.h has a default timeout of 3 minutes + 30 seconds.
/// We assume this is a reasonable timeout. We use half of that that.
/// https://github.com/SNeedlewoods/seraphis_wallet/blob/5f714f147fd29228698070e6bd80e41ce2f86fb0/src/wallet/wallet2.h#L238
static TIMEOUT: Duration = Duration::from_secs(3 * 60 + 30).checked_div(2).unwrap();

/// If the main node does not finish within this period, we start a hedged request.
static SOFT_TIMEOUT: Duration = TIMEOUT.checked_div(2).unwrap();

/// Trait alias for a stream that can be used with hyper
trait HyperStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> HyperStream for T {}

#[axum::debug_handler]
pub async fn proxy_handler(State(state): State<AppState>, request: Request) -> Response {
    static POOL_SIZE: usize = 20;

    // Get the pool of nodes
    let available_pool = state
        .node_pool
        .get_top_reliable_nodes(POOL_SIZE)
        .await
        .map_err(|e| HandlerError::PoolError(e.to_string()))
        .map(|nodes| {
            let pool: Vec<(String, String, u16)> = nodes
                .into_iter()
                .map(|node| (node.scheme, node.host, node.port))
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
    nodes: Vec<(String, String, u16)>,
) -> Result<Response, HandlerError> {
    if nodes.is_empty() {
        return Err(HandlerError::NoNodes);
    }

    // Sort nodes to prioritize those with available connections
    // Check if we're using Tor for this request
    let use_tor = match &state.tor_client {
        Some(tc)
            if tc.bootstrap_status().ready_for_traffic() && !request.clearnet_whitelisted() =>
        {
            true
        }
        _ => false,
    };

    // Create a vector of (node, has_connection) pairs
    let mut nodes_with_availability = Vec::new();
    for node in nodes.iter() {
        let key = (node.0.clone(), node.1.clone(), node.2, use_tor);
        let has_connection = state.connection_pool.has_available_connection(&key).await;
        nodes_with_availability.push((node.clone(), has_connection));
    }

    // Sort: nodes with available connections come first
    nodes_with_availability.sort_by(|a, b| {
        // If a has connection and b doesn't, a comes first
        // If both have or both don't have, maintain original order
        b.1.cmp(&a.1)
    });

    // Extract just the sorted nodes
    let nodes: Vec<(String, String, u16)> = nodes_with_availability
        .into_iter()
        .map(|(node, _)| node)
        .collect();

    let mut collected_errors: Vec<((String, String, u16), HandlerError)> = Vec::new();

    fn push_error(
        errors: &mut Vec<((String, String, u16), HandlerError)>,
        node: (String, String, u16),
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
    for pair in nodes.chunks(2) {
        let node = pair[0].clone();
        let next = pair.get(1).cloned();

        let node_uri = display_node(&node);

        // Start timing the request
        let latency = std::time::Instant::now();

        let mut winner = node.clone();
        let response = if let Some(hedge_node) = next.as_ref() {
            let hedge_node_uri = display_node(hedge_node);

            // Use hedged proxy: race node vs next
            match proxy_to_node_with_hedge(state, request.clone(), &node, hedge_node)
                .instrument(info_span!(
                    "connection",
                    node = node_uri,
                    hedge_node = hedge_node_uri,
                    tor = state.tor_client.is_some(),
                ))
                .await
            {
                Ok((response, winner_node)) => {
                    // Completed this pair; move on to next pair in iterator
                    winner = winner_node.clone();
                    response
                }
                Err(node_errors) => {
                    // One or both nodes failed; record the specific errors for each
                    for (failed_node, error) in node_errors {
                        push_error(
                            &mut collected_errors,
                            failed_node,
                            HandlerError::PhyiscalError(error),
                        );
                    }
                    continue;
                }
            }
        } else {
            // No hedge available; single node
            match proxy_to_single_node(state, request.clone(), &node)
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
            }
        };

        // Calculate the latency
        let latency = latency.elapsed().as_millis() as f64;

        // Fully buffer the response before forwarding it to the caller
        let buffered_response = CloneableResponse::from_response(response)
            .await
            .map_err(|e| {
                HandlerError::CloneRequestError(format!("Failed to buffer response: {}", e))
            })?;

        // Record total bytes for bandwidth statistics
        state
            .node_pool
            .record_bandwidth(buffered_response.body.len() as u64);

        let error = match buffered_response.get_jsonrpc_error() {
            Some(error) => {
                // Check if we have already got two previous JSON-RPC errors.
                // If we did, we assume there is a reason for it and return the response anyway.
                if collected_errors
                    .iter()
                    .filter(|(_, error)| matches!(error, HandlerError::JsonRpcError(_)))
                    .count()
                    >= 2
                {
                    return Ok(buffered_response.to_response());
                }

                Some(HandlerError::JsonRpcError(error))
            }
            None if buffered_response.status().is_client_error()
                || buffered_response.status().is_server_error() =>
            {
                Some(HandlerError::HttpError(buffered_response.status()))
            }
            _ => None,
        };

        match error {
            Some(error) => {
                push_error(&mut collected_errors, winner, error);
            }
            None => {
                tracing::trace!(
                    "Proxy request to {} succeeded, returning buffered response",
                    display_node(&winner)
                );

                // Only record errors if we have gotten a successful response
                // This helps prevent logging errors if it's likely our fault (e.g. no internet).
                for (node_failed, _) in collected_errors.iter() {
                    record_failure(&state, &node_failed.0, &node_failed.1, node_failed.2).await;
                }

                // Record the success with actual latency
                record_success(&state, &winner.0, &winner.1, winner.2, latency).await;

                // Return the buffered response (no streaming)
                return Ok(buffered_response.into_response());
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
        // Get root certificates for proper TLS verification
        let mut root_cert_store = tokio_rustls::rustls::RootCertStore::empty();
        root_cert_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

        // Create TLS client config with proper certificate validation
        let config = tokio_rustls::rustls::ClientConfig::builder()
            .with_root_certificates(root_cert_store)
            .with_no_client_auth();

        let connector = tokio_rustls::TlsConnector::from(std::sync::Arc::new(config));

        let server_name = host
            .to_string()
            .try_into()
            .map_err(|_| SingleRequestError::ConnectionError("Invalid DNS name".to_string()))?;

        let tls_stream = connector.connect(server_name, stream).await.map_err(|e| {
            SingleRequestError::ConnectionError(format!("TLS connection error: {:?}", e))
        })?;

        Ok(Box::new(tls_stream))
    } else {
        Ok(Box::new(stream))
    }
}

/// Result type for hedged requests that tracks individual node failures
type HedgedResult =
    Result<(Response, (String, String, u16)), Vec<((String, String, u16), SingleRequestError)>>;

/// Proxies a singular axum::Request to a given given main node with a specified hegde node
/// If the main nodes response hasn't finished after SOFT_TIMEOUT, we proxy to the hedge node
/// We then race the two responses, and return the one that finishes first (and is not an error)
async fn proxy_to_node_with_hedge(
    state: &crate::AppState,
    request: CloneableRequest,
    main_node: &(String, String, u16),
    hedge_node: &(String, String, u16),
) -> HedgedResult {
    use std::future::Future;

    // Start the main request immediately
    let mut main_fut = Box::pin(proxy_to_single_node(state, request.clone(), main_node));

    // Hedge request will be started after the soft timeout, unless the main fails first
    let mut hedge_fut: Option<
        Pin<Box<dyn Future<Output = Result<Response, SingleRequestError>> + Send>>,
    > = None;

    // Timer to trigger the hedge request
    let mut soft_timer = Box::pin(tokio::time::sleep(SOFT_TIMEOUT));
    let mut soft_timer_armed = true;

    // Track errors from both nodes
    let mut main_error: Option<SingleRequestError> = None;

    loop {
        // A future that awaits the hedge if present; otherwise stays pending
        let mut hedge_wait = futures::future::poll_fn(|cx| {
            if let Some(f) = hedge_fut.as_mut() {
                f.as_mut().poll(cx)
            } else {
                std::task::Poll::Pending
            }
        });

        tokio::select! {
            res = &mut main_fut => {
                match res {
                    Ok(resp) => return Ok((resp, main_node.clone())),
                    Err(err) => {
                        // Start hedge immediately if not yet started
                        main_error = Some(err);
                        if hedge_fut.is_none() {
                            tracing::debug!("Starting hedge request");
                            hedge_fut = Some(Box::pin(proxy_to_single_node(state, request.clone(), hedge_node)));
                        }

                        // If hedge exists, await it and prefer its result
                        if let Some(hf) = &mut hedge_fut {
                            match hf.await {
                                Ok(resp) => return Ok((resp, hedge_node.clone())),
                                Err(hedge_err) => {
                                    // Both failed, return errors for both nodes
                                    let mut errors = vec![(main_node.clone(), main_error.take().unwrap())];
                                    errors.push((hedge_node.clone(), hedge_err));
                                    return Err(errors);
                                }
                            }
                        } else {
                            // Only main was tried and failed
                            return Err(vec![(main_node.clone(), main_error.take().unwrap())]);
                        }
                    }
                }
            }

            // Start hedge after soft timeout if not already started
            _ = &mut soft_timer, if soft_timer_armed => {
                // Disarm timer so it does not keep firing
                soft_timer_armed = false;
                if hedge_fut.is_none() {
                    tracing::debug!("Starting hedge request");
                    hedge_fut = Some(Box::pin(proxy_to_single_node(state, request.clone(), hedge_node)));
                }
            }

            // If hedge is started, also race it
            res = &mut hedge_wait => {
                match res {
                    Ok(resp) => return Ok((resp, hedge_node.clone())),
                    Err(h_err) => {
                        // Hedge failed; if main already failed, return both errors
                        if let Some(m_err) = main_error.take() {
                            return Err(vec![
                                (main_node.clone(), m_err),
                                (hedge_node.clone(), h_err),
                            ]);
                        }
                        // Otherwise keep waiting on main
                        hedge_fut = None;
                    }
                }
            }
        }
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
    node: &(String, String, u16),
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
        // Build a new connection, and wrap it with TLS if needed.
        let address = (node.1.as_str(), node.2);

        let maybe_tls_stream = timeout(TIMEOUT, async {
            let no_tls_stream: Box<dyn HyperStream> = if use_tor {
                let tor_client = state.tor_client.as_ref().ok_or_else(|| {
                    SingleRequestError::ConnectionError("Tor requested but client missing".into())
                })?;

                let stream = tor_client
                    .connect(address)
                    .await
                    .map_err(|e| SingleRequestError::ConnectionError(format!("{:?}", e)))?;

                Box::new(stream)
            } else {
                let stream = TcpStream::connect(address)
                    .await
                    .map_err(|e| SingleRequestError::ConnectionError(format!("{:?}", e)))?;

                Box::new(stream)
            };

            maybe_wrap_with_tls(no_tls_stream, &node.0, &node.1).await
        })
        .await
        .map_err(|_| SingleRequestError::Timeout("Connection timed out".to_string()))??;

        let maybe_tls_stream = TokioIo::new(maybe_tls_stream);

        // Build an HTTP/1 connection over the stream.
        let (sender, conn) = hyper::client::conn::http1::handshake(maybe_tls_stream)
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
    // Buffer the entire response to avoid "end of file before message length reached" errors
    let (parts, body) = response.into_parts();

    // Collect the entire body into memory
    let body_bytes = match body.collect().await {
        Ok(collected) => collected.to_bytes().to_vec(),
        Err(e) => {
            // If we fail to read the full body, mark connection as failed
            guarded_sender.mark_failed().await;
            return Err(SingleRequestError::SendRequestError(format!(
                "Failed to read response body: {}",
                e
            )));
        }
    };

    let axum_body = Body::from(body_bytes);
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

    // If we can't parse JSON, don't treat it as an error
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
    AllRequestsFailed(Vec<((String, String, u16), HandlerError)>),
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

fn display_node(node: &(String, String, u16)) -> String {
    format!("{}://{}:{}", node.0, node.1, node.2)
}

async fn record_success(state: &AppState, scheme: &str, host: &str, port: u16, latency_ms: f64) {
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

async fn record_failure(state: &AppState, scheme: &str, host: &str, port: u16) {
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
