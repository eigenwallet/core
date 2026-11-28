//! Shared parts of the price sources that poll a REST API.

use futures::StreamExt;
use futures::stream::{self, BoxStream};
use serde::de::DeserializeOwned;
use std::convert::Infallible;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

const POLL_INTERVAL: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

const RETRY_DELAY_INITIAL: Duration = Duration::from_secs(3);
const RETRY_DELAY_MAX: Duration = Duration::from_secs(60);

const MAX_BODY_BYTES: usize = 1024 * 1024;
const MAX_ERROR_BODY_CHARS: usize = 256;

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("HTTP request failed")]
    Request(#[source] reqwest::Error),
    #[error("Failed to read the response body")]
    BodyRead(#[source] reqwest::Error),
    #[error("Response body is larger than {MAX_BODY_BYTES} bytes")]
    BodyTooLarge,
    #[error("Server returned non-success status {status}: {body}")]
    Status {
        status: reqwest::StatusCode,
        body: String,
    },
    #[error("Failed to decode the JSON response")]
    Decode(#[source] serde_json::Error),
}

/// Call `fetch` on a fixed interval and yield each update.
///
/// A failed poll is retried here and never ends the stream.
/// The backoff of [`crate::ticker::connect`] does not reset after an update,
/// so a stream error would delay the next poll more and more.
/// Until a retry succeeds, subscribers keep the last update and its timestamp.
pub(crate) fn poll<Params, Update, Error, FetchRet>(
    source: &'static str,
    params: Arc<Params>,
    fetch: fn(Arc<Params>) -> FetchRet,
) -> BoxStream<'static, Result<Update, Infallible>>
where
    Params: Send + Sync + 'static,
    Update: Send + 'static,
    Error: std::error::Error + Send + Sync + 'static,
    FetchRet: Future<Output = Result<Update, Error>> + Send + 'static,
{
    stream::unfold((params, true), move |(params, first)| async move {
        if !first {
            tokio::time::sleep(POLL_INTERVAL).await;
        }

        let update = fetch_until_success(source, &params, fetch).await;

        Some((Ok(update), (params, false)))
    })
    .boxed()
}

async fn fetch_until_success<Params, Update, Error, FetchRet>(
    source: &'static str,
    params: &Arc<Params>,
    fetch: fn(Arc<Params>) -> FetchRet,
) -> Update
where
    Error: std::error::Error + Send + Sync + 'static,
    FetchRet: Future<Output = Result<Update, Error>>,
{
    let mut attempt: u64 = 1;
    let mut retry_delay = RETRY_DELAY_INITIAL;

    loop {
        let error = match fetch(params.clone()).await {
            Ok(update) => return update,
            Err(error) => error,
        };

        tracing::warn!(
            source,
            attempt,
            retry_in_secs = retry_delay.as_secs(),
            error = %format!("{:#}", anyhow::Error::from(error)),
            "Failed to poll price source. Retrying",
        );

        tokio::time::sleep(retry_delay).await;
        attempt += 1;
        retry_delay = (retry_delay * 2).min(RETRY_DELAY_MAX);
    }
}

/// Send the request and decode the JSON response.
/// The request has a timeout and the response body a size limit.
pub(crate) async fn fetch_json<T: DeserializeOwned>(
    request: reqwest::RequestBuilder,
) -> Result<T, FetchError> {
    let mut response = request
        .timeout(REQUEST_TIMEOUT)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(FetchError::Request)?;

    let status = response.status();
    let body = read_body(&mut response).await?;

    if !status.is_success() {
        let body = String::from_utf8_lossy(&body)
            .chars()
            .take(MAX_ERROR_BODY_CHARS)
            .collect();
        return Err(FetchError::Status { status, body });
    }

    serde_json::from_slice(&body).map_err(FetchError::Decode)
}

async fn read_body(response: &mut reqwest::Response) -> Result<Vec<u8>, FetchError> {
    let mut body = Vec::new();

    while let Some(chunk) = response.chunk().await.map_err(FetchError::BodyRead)? {
        if body.len() + chunk.len() > MAX_BODY_BYTES {
            return Err(FetchError::BodyTooLarge);
        }
        body.extend_from_slice(&chunk);
    }

    Ok(body)
}
