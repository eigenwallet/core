//! Async, multi-server Electrum client pool built on [`electrum_streaming_client`].
//!
//! [`ElectrumBalancer`] owns one lazily-established [`Connection`] per server URL and runs
//! operations against them with sticky round-robin failover: it stays on the current server while
//! it succeeds and advances to the next on error, retrying with exponential backoff until every
//! server has been tried at least once (or `min_retries`, whichever is larger).

mod connection;

pub use connection::Connection;

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use bitcoin::Transaction;
use electrum_streaming_client::request::BroadcastTx;
use electrum_streaming_client::{RequestExt, response};
use futures::future::BoxFuture;
use tokio::sync::Mutex;
use tracing::{debug, instrument, trace, warn};

/// Error from a single Electrum operation against one server.
#[derive(Debug, Clone)]
pub enum Error {
    /// Transport/connection-level failure. The balancer fails over and drops the connection so it
    /// is re-established on next use.
    Connection(String),
    /// The Electrum server returned a JSON-RPC error. Holds the raw error JSON payload as text so
    /// that callers (e.g. RPC error-code parsing) can inspect it.
    Response(String),
}

impl Error {
    pub fn connection(msg: impl Into<String>) -> Self {
        Error::Connection(msg.into())
    }

    /// Build a [`Error::Response`] from the streaming client's server error, extracting the raw
    /// JSON payload (its `Display` is `"Response.error: <json>"`).
    pub fn response(err: &electrum_streaming_client::ResponseError) -> Self {
        let text = err.to_string();
        let json = text
            .strip_prefix("Response.error: ")
            .unwrap_or(&text)
            .to_string();
        Error::Response(json)
    }

    /// Whether this is a transport-level failure that warrants reconnecting the server.
    pub fn is_connection(&self) -> bool {
        matches!(self, Error::Connection(_))
    }

    /// The raw server error JSON payload, if this is a server response error.
    pub fn response_json(&self) -> Option<&str> {
        match self {
            Error::Response(json) => Some(json),
            Error::Connection(_) => None,
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Connection(msg) => write!(f, "Electrum connection error: {msg}"),
            Error::Response(json) => write!(f, "Electrum server error: {json}"),
        }
    }
}

impl std::error::Error for Error {}

/// Factory that establishes a connection to a server URL.
pub trait ConnectionFactory<C>: Send + Sync {
    fn connect(&self, url: String, request_timeout: Duration) -> BoxFuture<'static, Result<C, Error>>;
}

/// Default factory producing real [`Connection`]s.
pub struct DefaultConnectionFactory;

impl ConnectionFactory<Connection> for DefaultConnectionFactory {
    fn connect(
        &self,
        url: String,
        request_timeout: Duration,
    ) -> BoxFuture<'static, Result<Connection, Error>> {
        Box::pin(async move { Connection::connect(&url, request_timeout).await })
    }
}

/// Configuration for the Electrum balancer.
#[derive(Clone, Debug)]
pub struct ElectrumBalancerConfig {
    /// Per-request (and per-connect) timeout.
    pub request_timeout: Duration,
    /// Minimum number of attempts across all servers before giving up.
    pub min_retries: usize,
}

impl Default for ElectrumBalancerConfig {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(15),
            min_retries: 10,
        }
    }
}

struct ConnectionSlot<C> {
    connection: Mutex<Option<Arc<C>>>,
}

impl<C> ConnectionSlot<C> {
    fn new() -> Self {
        Self {
            connection: Mutex::new(None),
        }
    }
}

/// Sticky round-robin load balancer over one [`Connection`] per Electrum server.
pub struct ElectrumBalancer<C = Connection> {
    urls: Vec<String>,
    slots: Arc<Vec<ConnectionSlot<C>>>,
    next: AtomicUsize,
    config: ElectrumBalancerConfig,
    factory: Arc<dyn ConnectionFactory<C>>,
}

impl ElectrumBalancer<Connection> {
    /// Create a balancer over the given URLs with default configuration.
    pub fn new(urls: Vec<String>) -> Result<Self, Error> {
        Self::new_with_config(urls, ElectrumBalancerConfig::default())
    }

    /// Create a balancer over the given URLs with custom configuration.
    pub fn new_with_config(
        urls: Vec<String>,
        config: ElectrumBalancerConfig,
    ) -> Result<Self, Error> {
        Self::new_with_factory(urls, config, Arc::new(DefaultConnectionFactory))
    }
}

impl<C> ElectrumBalancer<C>
where
    C: Send + Sync + 'static,
{
    /// Create a balancer from a connection factory. Connections are established lazily on first use.
    pub fn new_with_factory(
        urls: Vec<String>,
        config: ElectrumBalancerConfig,
        factory: Arc<dyn ConnectionFactory<C>>,
    ) -> Result<Self, Error> {
        if urls.is_empty() {
            return Err(Error::connection("No Electrum URLs provided"));
        }

        debug!(
            servers = ?urls,
            server_count = urls.len(),
            timeout_ms = config.request_timeout.as_millis(),
            min_retries = config.min_retries,
            "Initializing Electrum load balancer"
        );

        let slots = (0..urls.len()).map(|_| ConnectionSlot::new()).collect();

        Ok(Self {
            urls,
            slots: Arc::new(slots),
            next: AtomicUsize::new(0),
            config,
            factory,
        })
    }

    /// The configured server URLs.
    pub fn urls(&self) -> &Vec<String> {
        &self.urls
    }

    /// The number of servers in the pool.
    pub fn server_count(&self) -> usize {
        self.urls.len()
    }

    /// The balancer configuration.
    pub fn config(&self) -> &ElectrumBalancerConfig {
        &self.config
    }

    async fn get_or_connect(&self, idx: usize) -> Result<Arc<C>, Error> {
        let mut guard = self.slots[idx].connection.lock().await;
        if let Some(connection) = guard.as_ref() {
            return Ok(connection.clone());
        }

        let connection = self
            .factory
            .connect(self.urls[idx].clone(), self.config.request_timeout)
            .await?;
        let connection = Arc::new(connection);
        *guard = Some(connection.clone());
        Ok(connection)
    }

    async fn invalidate(&self, idx: usize) {
        *self.slots[idx].connection.lock().await = None;
    }

    fn advance(&self) {
        let count = self.urls.len();
        let _ = self
            .next
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
                Some((current + 1) % count)
            });
    }

    /// Run the operation against a single server, failing over to the next on error.
    ///
    /// Stays on the last successful server (sticky) and advances on failure. Retries with
    /// exponential backoff up to `max(min_retries, server_count)` attempts before returning a
    /// [`MultiError`] aggregating every failure.
    #[instrument(level = "debug", skip(self, f), fields(operation = kind, servers = self.urls.len()))]
    pub async fn run<F, Fut, T>(&self, kind: &str, f: F) -> Result<T, MultiError>
    where
        F: Fn(Arc<C>) -> Fut + Send + Sync,
        Fut: Future<Output = Result<T, Error>> + Send,
        T: Send,
    {
        let allowed = std::cmp::max(self.config.min_retries, self.urls.len());
        let mut errors = Vec::new();
        let mut backoff = Duration::from_millis(100);

        while errors.len() < allowed {
            let idx = self.next.load(Ordering::SeqCst);

            let connection = match self.get_or_connect(idx).await {
                Ok(connection) => connection,
                Err(e) => {
                    trace!(server_url = self.urls[idx], error = %e, "Connection failed, trying next");
                    errors.push(e);
                    self.advance();
                    Self::sleep_backoff(&mut backoff).await;
                    continue;
                }
            };

            match f(connection).await {
                Ok(value) => return Ok(value),
                Err(e) => {
                    trace!(server_url = self.urls[idx], error = %e, "Operation failed, trying next");
                    if e.is_connection() {
                        self.invalidate(idx).await;
                    }
                    errors.push(e);
                    self.advance();
                    if errors.len() < allowed {
                        Self::sleep_backoff(&mut backoff).await;
                    }
                }
            }
        }

        warn!(
            operation = kind,
            attempts = errors.len(),
            servers = self.urls.len(),
            "All Electrum servers failed after exhausting retries"
        );

        Err(MultiError::new(
            errors,
            format!(
                "All {} Electrum servers failed after {} attempts for operation '{}'",
                self.urls.len(),
                self.urls.len(),
                kind
            ),
        ))
    }

    /// Run the operation against every server concurrently, returning one result per server in URL
    /// order.
    #[instrument(level = "debug", skip(self, f), fields(operation = kind, servers = self.urls.len()))]
    pub async fn join_all<F, Fut, T>(&self, kind: &str, f: F) -> Vec<Result<T, Error>>
    where
        F: Fn(Arc<C>) -> Fut + Send + Sync,
        Fut: Future<Output = Result<T, Error>> + Send,
        T: Send,
    {
        let tasks = (0..self.urls.len()).map(|idx| {
            let f = &f;
            async move {
                let connection = self.get_or_connect(idx).await?;
                let result = f(connection).await;
                if let Err(e) = &result {
                    if e.is_connection() {
                        self.invalidate(idx).await;
                    }
                }
                result
            }
        });

        futures::future::join_all(tasks).await
    }

    async fn sleep_backoff(backoff: &mut Duration) {
        tokio::time::sleep(*backoff).await;
        *backoff = std::cmp::min(backoff.mul_f64(1.5), Duration::from_millis(1500));
    }
}

impl ElectrumBalancer<Connection> {
    /// Issue a typed request against a single server with failover.
    pub async fn request<Req>(&self, kind: &str, req: Req) -> Result<Req::Response, MultiError>
    where
        Req: RequestExt + Clone + Send + Sync + 'static,
        Req::Response: Send,
    {
        self.run(kind, move |connection| {
            let req = req.clone();
            async move { connection.request(req).await }
        })
        .await
    }

    /// Issue a typed request against every server concurrently.
    pub async fn request_join_all<Req>(&self, kind: &str, req: Req) -> Vec<Result<Req::Response, Error>>
    where
        Req: RequestExt + Clone + Send + Sync + 'static,
        Req::Response: Send,
    {
        self.join_all(kind, move |connection| {
            let req = req.clone();
            async move { connection.request(req).await }
        })
        .await
    }

    /// Broadcast a transaction to every server concurrently. Returns one result per server.
    pub async fn broadcast_all(&self, tx: Transaction) -> Vec<Result<bitcoin::Txid, Error>> {
        self.request_join_all("transaction_broadcast", BroadcastTx(tx))
            .await
    }

    /// Fetch a single script's history from every server concurrently.
    pub async fn script_get_history_all(
        &self,
        script: bitcoin::ScriptBuf,
    ) -> Vec<Result<Vec<response::Tx>, Error>> {
        use electrum_streaming_client::request::GetHistory;
        self.request_join_all("script_get_history", GetHistory::from_script(script))
            .await
    }
}

/// Aggregates the per-server failures of a balancer operation.
///
/// Part of the public API: consumed by RPC error-code parsing and broadcast result handling.
#[derive(Debug, Clone)]
pub struct MultiError {
    pub errors: Vec<Error>,
    pub context: String,
}

impl MultiError {
    pub fn new(errors: Vec<Error>, context: impl Into<String>) -> Self {
        Self {
            errors,
            context: context.into(),
        }
    }

    pub fn len(&self) -> usize {
        self.errors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Error> {
        self.errors.iter()
    }

    pub fn any<F>(&self, predicate: F) -> bool
    where
        F: Fn(&Error) -> bool,
    {
        self.errors.iter().any(predicate)
    }

    pub fn all<F>(&self, predicate: F) -> bool
    where
        F: Fn(&Error) -> bool,
    {
        self.errors.iter().all(predicate)
    }
}

impl std::fmt::Display for MultiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {} errors occurred", self.context, self.errors.len())?;
        for (i, error) in self.errors.iter().enumerate() {
            write!(f, "\n  {}: {}", i + 1, error)?;
        }
        Ok(())
    }
}

impl std::error::Error for MultiError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    struct MockConnection {
        url: String,
        calls: Arc<AtomicUsize>,
        outcome: MockOutcome,
    }

    #[derive(Clone, Copy)]
    enum MockOutcome {
        Ok,
        ConnectionError,
        ResponseError,
    }

    impl MockConnection {
        async fn call(self: Arc<Self>) -> Result<String, Error> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match self.outcome {
                MockOutcome::Ok => Ok(self.url.clone()),
                MockOutcome::ConnectionError => Err(Error::connection(format!("io {}", self.url))),
                MockOutcome::ResponseError => {
                    Err(Error::Response(format!("{{\"code\":-5}} {}", self.url)))
                }
            }
        }
    }

    struct MockFactory {
        outcomes: std::collections::HashMap<String, MockOutcome>,
        calls: std::sync::Mutex<std::collections::HashMap<String, Arc<AtomicUsize>>>,
    }

    impl MockFactory {
        fn new(outcomes: Vec<(&str, MockOutcome)>) -> Arc<Self> {
            Arc::new(Self {
                outcomes: outcomes
                    .into_iter()
                    .map(|(u, o)| (u.to_string(), o))
                    .collect(),
                calls: std::sync::Mutex::new(std::collections::HashMap::new()),
            })
        }

        fn call_count(&self, url: &str) -> usize {
            self.calls
                .lock()
                .unwrap()
                .get(url)
                .map(|c| c.load(Ordering::SeqCst))
                .unwrap_or(0)
        }
    }

    impl ConnectionFactory<MockConnection> for MockFactory {
        fn connect(
            &self,
            url: String,
            _request_timeout: Duration,
        ) -> BoxFuture<'static, Result<MockConnection, Error>> {
            let outcome = self.outcomes.get(&url).copied().unwrap_or(MockOutcome::Ok);
            let calls = self
                .calls
                .lock()
                .unwrap()
                .entry(url.clone())
                .or_insert_with(|| Arc::new(AtomicUsize::new(0)))
                .clone();
            Box::pin(async move { Ok(MockConnection { url, calls, outcome }) })
        }
    }

    fn fast_config() -> ElectrumBalancerConfig {
        ElectrumBalancerConfig {
            request_timeout: Duration::from_secs(1),
            min_retries: 0,
        }
    }

    #[tokio::test(start_paused = true)]
    async fn empty_urls_is_error() {
        let factory = MockFactory::new(vec![]);
        let balancer = ElectrumBalancer::new_with_factory(vec![], fast_config(), factory);
        assert!(balancer.is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn sticky_stays_on_first_server() {
        let urls = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let factory = MockFactory::new(vec![
            ("a", MockOutcome::Ok),
            ("b", MockOutcome::Ok),
            ("c", MockOutcome::Ok),
        ]);
        let balancer =
            ElectrumBalancer::new_with_factory(urls, fast_config(), factory.clone()).unwrap();

        for _ in 0..5 {
            let result = balancer.run("test", |c| async move { c.call().await }).await;
            assert!(result.is_ok());
        }

        assert_eq!(factory.call_count("a"), 5);
        assert_eq!(factory.call_count("b"), 0);
        assert_eq!(factory.call_count("c"), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn fails_over_on_error() {
        let urls = vec!["a".to_string(), "b".to_string()];
        let factory = MockFactory::new(vec![
            ("a", MockOutcome::ConnectionError),
            ("b", MockOutcome::Ok),
        ]);
        let balancer =
            ElectrumBalancer::new_with_factory(urls, fast_config(), factory.clone()).unwrap();

        let result = balancer.run("test", |c| async move { c.call().await }).await;
        assert_eq!(result.unwrap(), "b");
        assert_eq!(factory.call_count("a"), 1);
        assert_eq!(factory.call_count("b"), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn all_fail_yields_multi_error() {
        let urls = vec!["a".to_string(), "b".to_string()];
        let factory = MockFactory::new(vec![
            ("a", MockOutcome::ResponseError),
            ("b", MockOutcome::ResponseError),
        ]);
        let balancer =
            ElectrumBalancer::new_with_factory(urls, fast_config(), factory).unwrap();

        let result = balancer.run("test", |c| async move { c.call().await }).await;
        let err = result.unwrap_err();
        assert!(err.len() >= 2);
        assert!(err.any(|e| e.response_json().is_some_and(|j| j.contains("-5"))));
    }

    #[tokio::test(start_paused = true)]
    async fn join_all_hits_every_server() {
        let urls = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let factory = MockFactory::new(vec![
            ("a", MockOutcome::Ok),
            ("b", MockOutcome::ConnectionError),
            ("c", MockOutcome::Ok),
        ]);
        let balancer =
            ElectrumBalancer::new_with_factory(urls, fast_config(), factory.clone()).unwrap();

        let results = balancer.join_all("test", |c| async move { c.call().await }).await;
        assert_eq!(results.len(), 3);
        assert!(results[0].is_ok());
        assert!(results[1].is_err());
        assert!(results[2].is_ok());
        assert_eq!(factory.call_count("a"), 1);
        assert_eq!(factory.call_count("b"), 1);
        assert_eq!(factory.call_count("c"), 1);
    }
}
