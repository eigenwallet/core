use axum::{
    body::Body,
    extract::{Request, State},
    http::StatusCode,
    response::Response,
};
use http_body_util::BodyExt;
use tracing::{error, info_span, Instrument};

use crate::AppState;

/// Proxies a singular axum::Request to a single node.
/// Errors if we get a physical connection error
/// Does NO error if the response is a HTTP error or a JSON-RPC error
/// The caller is responsible for checking the response status and body for errors
async fn proxy_to_single_node(
    request: Request,
    node: &(String, String, i64),
) -> Result<Response, SingleRequestError> {
    use hyper_util::rt::TokioIo;
    use tokio::net::TcpStream;

    // Open a TCP connection to a random node in the pool
    let stream = TcpStream::connect(format!("{}:{}", node.1, node.2))
        .await
        .map_err(|e| SingleRequestError::ConnectionError(e.to_string()))?;
    let io = TokioIo::new(stream);

    // Create the Hyper client for that node
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
        .await
        // TODO: When exactly does this error?
        .map_err(|e| SingleRequestError::ConnectionError(e.to_string()))?;

    // Spawn a task to poll the connection, driving the HTTP state
    // TODO: Put this into a connection pool
    tokio::task::spawn(async move {
        if let Err(err) = conn.await {
            println!("Connection failed: {:?}", err);
        }
    });

    // Forward the request to the node
    // No need to rewrite the URI because the request.uri() is relative
    let response = sender
        .send_request(request)
        .await
        .map_err(|e| SingleRequestError::SendRequestError(e.to_string()))?;

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
        return json.get("error").and_then(|e| e.as_str().map(|s| s.to_string()));
    }

    // If we can't parse JSON, treat it as an error
    None
}   

/// axum::Request is not cloneable by itself
async fn clone_request(resp: Request<Body>) -> Result<(Request<Body>, Request<Body>), axum::Error> {
    let (parts, body) = resp.into_parts();
    
    let body_bytes_source = body.collect().await?.to_bytes();

    let body_parts = Body::from(body_bytes_source.clone());
    let body_parts_clone = Body::from(body_bytes_source.clone());

    let resp1 = Request::from_parts(parts.clone(), body_parts);
    let resp2 = Request::from_parts(parts.clone(), body_parts_clone);

    Ok((resp1, resp2))
}

/// Given a Vec of nodes, proxy the given request to multiple nodes until we get a successful response
async fn proxy_to_multiple_nodes(
    state: &AppState,
    mut request: Request,
    nodes: Vec<(String, String, i64)>,
) -> Result<Response, HandlerError> {
    if nodes.is_empty() {
        return Err(HandlerError::NoNodes);
    }

    let mut collected_errors: Vec<((String, String, i64), HandlerError)> = Vec::new();

    fn push_error(errors: &mut Vec<((String, String, i64), HandlerError)>, node: (String, String, i64), error: HandlerError) {
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

        let (original_request, cloned_request) = clone_request(request).await.map_err(|e| HandlerError::CloneRequestError(e.to_string()))?;
        request = original_request;
        
        let response = match proxy_to_single_node(cloned_request, &node).await {
            Ok(response) => response,
            Err(e) => {
                push_error(&mut collected_errors, node, HandlerError::PhyiscalError(e));
                continue;
            }
        };

        let response_status = response.status();
        let (response_parts, response_body) = response.into_parts();
        let response_body = response_body.collect().await.expect("response body should be collectable").to_bytes();

        let response_clone = Response::from_parts(response_parts.clone(), Body::from(response_body.clone()));

        let error = match get_jsonrpc_error(&response_body) {
            Some(error) => {
                // Check if we have already got two previous JSON-RPC errors
                // If we did, we assume there is a reason for it
                // We return the response as is.
                if collected_errors.iter().filter(|(_, error)| matches!(error, HandlerError::JsonRpcError(_))).count() >= 2 {
                    return Ok(response_clone);
                }

                Some(HandlerError::JsonRpcError(error))
            },
            None if response_status.is_client_error() || response_status.is_server_error() => {                
                Some(HandlerError::HttpError(response_status))
            }
            _ => None
        };

        match error {
            Some(error) => {
                tracing::info!("Proxy request to {} failed: {}", node_uri, error);
                push_error(&mut collected_errors, node, error);
            }
            None => {
                tracing::info!("Proxy request to {} succeeded with size {}kb", node_uri, response_body.len() / 1024);
                // Only record errors if we have gotten a successful response
                // This helps prevent logging errors if its our likely our fault (no internet)
                for (node, _) in collected_errors.iter() {
                    record_failure(&state, &node.0, &node.1, node.2).await;
                }

                // Record the success
                record_success(&state, &node.0, &node.1, node.2, 0.0).await;
                
                // Finally return the successful response
                return Ok(response_clone);
            }
        }
    }

    Err(HandlerError::AllRequestsFailed(collected_errors))
}

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

    let uri = request.uri().to_string();
    match proxy_to_multiple_nodes(&state, request, available_pool)
        .instrument(info_span!("request", uri = uri))
        .await
    {
        Ok(response) => response,
        Err(error) => error.to_response(),
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
            HandlerError::CloneRequestError(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Request processing error"),
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
            SingleRequestError::CollectResponseError(msg) => write!(f, "Collect response error: {}", msg),
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
                    "top_reliable_nodes": status.top_reliable_nodes
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
