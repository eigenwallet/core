//! libp2p Prometheus metrics for the ASB.
//!
//! [`MetricsServer`] exposes a [`Registry`] over HTTP at `/metrics`. The
//! [`Metrics`] recorder (driven from the event loop) and the bandwidth
//! transport wrapper both register into that registry.

use anyhow::{Context, Result};
use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use hyper::header::CONTENT_TYPE;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use prometheus_client::encoding::text::encode;
use std::convert::Infallible;
use std::net::Ipv4Addr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_util::task::AbortOnDropHandle;

pub use libp2p::metrics::{Metrics, Registry};

/// OpenMetrics content type emitted by [`prometheus_client`]'s text encoder.
const METRICS_CONTENT_TYPE: &str = "application/openmetrics-text; version=1.0.0; charset=utf-8";

#[allow(missing_debug_implementations)]
pub struct MetricsServer;

impl MetricsServer {
    /// Binds the metrics HTTP server on `0.0.0.0:port` and serves the registry
    /// at `/metrics`. The returned handle aborts the server when dropped.
    pub async fn start(port: u16, registry: Registry) -> Result<AbortOnDropHandle<()>> {
        let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port))
            .await
            .with_context(|| format!("Failed to bind Prometheus metrics server on port {port}"))?;
        let addr = listener.local_addr()?;

        let registry = Arc::new(registry);

        tracing::info!(%addr, "Prometheus metrics server listening");

        let handle = tokio::spawn(async move {
            loop {
                let (stream, _) = match listener.accept().await {
                    Ok(connection) => connection,
                    Err(error) => {
                        tracing::warn!(%error, "Failed to accept metrics connection");
                        continue;
                    }
                };

                let registry = registry.clone();
                tokio::spawn(async move {
                    let service =
                        service_fn(move |request| handle_request(request, registry.clone()));

                    if let Err(error) = http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), service)
                        .await
                    {
                        tracing::debug!(%error, "Metrics connection closed with error");
                    }
                });
            }
        });

        Ok(AbortOnDropHandle::new(handle))
    }
}

async fn handle_request(
    request: Request<Incoming>,
    registry: Arc<Registry>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    if request.uri().path() != "/metrics" {
        return Ok(empty_response(StatusCode::NOT_FOUND));
    }

    let mut buffer = String::new();
    if let Err(error) = encode(&mut buffer, &registry) {
        tracing::error!(%error, "Failed to encode Prometheus metrics");
        return Ok(empty_response(StatusCode::INTERNAL_SERVER_ERROR));
    }

    let response = Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, METRICS_CONTENT_TYPE)
        .body(Full::new(Bytes::from(buffer)))
        .expect("metrics response to be valid");

    Ok(response)
}

fn empty_response(status: StatusCode) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .body(Full::new(Bytes::new()))
        .expect("empty response to be valid")
}
