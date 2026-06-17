use bdk_electrum::BdkElectrumClient;
use bdk_electrum::electrum_client::{Client, ConfigBuilder, ElectrumApi, Error};
use bitcoin::Transaction;
use futures::stream::{FuturesUnordered, StreamExt};
use once_cell::sync::OnceCell;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use std::time::Instant;
use tokio::task::spawn_blocking;
use tracing::{debug, error, instrument, trace, warn};

/// Failover pool for Electrum connections.
///
/// The first URL is considered the primary server. Every sequential
/// request starts at the primary and fails over through the remaining
/// servers in order until the provided closure succeeds or all servers
/// have been exhausted. Failing over to the next server happens
/// immediately; a backoff is only applied once a full pass over all
/// servers has failed.
///
/// Sequential calls fail over on *every* error, including protocol
/// errors, because those may be server deficiencies ("txindex not
/// ready", unsupported endpoint) that another server does not have.
///
/// Parallel quorum operations always wait for the primary's result.
///
/// Clients are created lazily on first use to avoid blocking during initialization.
pub struct ElectrumBalancer<C = BdkElectrumClient<Client>>
where
    C: ElectrumClientLike,
{
    urls: Vec<String>,
    #[allow(clippy::type_complexity)]
    clients: Arc<RwLock<Vec<Arc<OnceCell<Arc<C>>>>>>,
    config: ElectrumBalancerConfig,
    factory: Arc<dyn ElectrumClientFactory<C> + Send + Sync>,
}

impl<C> ElectrumBalancer<C>
where
    C: ElectrumClientLike,
{
    /// Helper function to get or initialize a client for a given index
    fn get_or_init_client_sync(&self, idx: usize) -> Result<Arc<C>, Error> {
        // We wrap this in a closure to only lock the RwLock for as long as needed
        let (client_once_cell, url, config, factory) = {
            let clients = self.clients.read().expect("rwlock poisoned").clone();

            if idx >= clients.len() {
                return Err(Error::IOError(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("Index {} out of bounds for {} clients", idx, clients.len()),
                )));
            }

            let once_cell = clients[idx].clone();
            let url = self.urls[idx].clone();
            let config = self.config.clone();
            let factory = self.factory.clone();

            (once_cell, url, config, factory)
        };

        let client = client_once_cell.get_or_try_init(|| factory.create_client(&url, &config))?;

        Ok(client.clone())
    }

    async fn get_or_init_client_async(&self, idx: usize) -> Result<Arc<C>, Error> {
        let balancer = self.clone();
        spawn_blocking(move || balancer.get_or_init_client_sync(idx))
            .await
            .map_err(|e| Error::IOError(std::io::Error::other(e.to_string())))?
    }

    /// Create a new balancer from a list of Electrum URLs with default configuration.
    pub async fn new_with_factory(
        urls: Vec<String>,
        factory: Arc<dyn ElectrumClientFactory<C> + Send + Sync>,
    ) -> Result<Self, Error> {
        Self::new_with_config_and_factory(urls, ElectrumBalancerConfig::default(), factory).await
    }

    /// Get any client from the balancer
    pub async fn get_any_client(&self) -> Result<Arc<C>, Error> {
        // Try to initialize any client
        for idx in 0..self.client_count() {
            match self.get_or_init_client_async(idx).await {
                Ok(client) => return Ok(client),
                Err(e) => {
                    trace!(
                        server_url = self.urls[idx],
                        error = ?e,
                        "Failed to initialize client, trying next client"
                    );
                }
            }
        }

        // Return error if no client could be initialized
        Err(Error::IOError(std::io::Error::other(
            "No client could be initialized",
        )))
    }

    /// Create a new balancer from a list of Electrum URLs with custom configuration.
    /// Clients are initialized lazily on first use.
    pub async fn new_with_config_and_factory(
        urls: Vec<String>,
        config: ElectrumBalancerConfig,
        factory: Arc<dyn ElectrumClientFactory<C> + Send + Sync>,
    ) -> Result<Self, Error> {
        if urls.is_empty() {
            return Err(Error::Protocol("No Electrum URLs provided".into()));
        }

        debug!(
            servers = ?urls,
            server_count = urls.len(),
            timeout_seconds = config.request_timeout,
            min_retries = config.min_retries,
            "Initializing Electrum load balancer"
        );

        // Create OnceCell containers for each URL - clients will be created on first use
        let clients: Vec<Arc<OnceCell<Arc<C>>>> =
            urls.iter().map(|_| Arc::new(OnceCell::new())).collect();

        Ok(Self {
            urls,
            clients: Arc::new(RwLock::new(clients)),
            config,
            factory,
        })
    }

    /// Get the number of URLs (potential clients)
    pub fn client_count(&self) -> usize {
        self.urls.len()
    }

    /// Execute the given closure using one of the Electrum clients asynchronously.
    ///
    /// If the closure returns an I/O error or certificate error the balancer will try the next
    /// node until all nodes have been exhausted. The last encountered error
    /// is returned in that case.
    #[instrument(level = "debug", skip(self, f), fields(operation = kind, total_urls = self.urls.len(), total_clients = self.client_count()))]
    pub async fn call<F, T>(&self, kind: &str, f: F) -> Result<T, Error>
    where
        F: Fn(&C) -> Result<T, Error> + Send + Sync + Clone + 'static,
        T: Send + 'static,
    {
        let balancer = self.clone();
        let kind = kind.to_string();

        match spawn_blocking(move || balancer.call_sync(&kind, f)).await {
            Ok(result) => result.map_err(|multi_error| multi_error.into()),
            Err(e) => Err(Error::IOError(std::io::Error::other(e.to_string()))),
        }
    }

    /// Execute the given closure using one of the Electrum clients asynchronously.
    ///
    /// If the closure returns an I/O error or certificate error the balancer will try the next
    /// node until all nodes have been exhausted. The last encountered error
    /// is returned in that case.
    #[instrument(level = "debug", skip(self, f), fields(operation = kind, total_urls = self.urls.len(), total_clients = self.client_count()))]
    pub async fn call_async<F, T>(&self, kind: &str, f: F) -> Result<T, Error>
    where
        F: Fn(&C) -> Result<T, Error> + Send + Sync + Clone + 'static,
        T: Send + 'static,
    {
        let balancer = self.clone();
        let kind = kind.to_string();

        match spawn_blocking(move || balancer.call_sync(&kind, f)).await {
            Ok(result) => result.map_err(|multi_error| multi_error.into()),
            Err(e) => Err(Error::IOError(std::io::Error::other(e.to_string()))),
        }
    }

    /// Execute the given closure using one of the Electrum clients asynchronously,
    /// returning the full MultiError for detailed error analysis.
    ///
    /// Unlike `call_async`, this method exposes the full MultiError containing all
    /// individual failures, allowing the caller to inspect and make decisions based
    /// on the specific types of errors encountered.
    #[instrument(level = "debug", skip(self, f), fields(operation = kind, total_clients = self.client_count()))]
    pub async fn call_async_with_multi_error<F, T>(&self, kind: &str, f: F) -> Result<T, MultiError>
    where
        F: Fn(&C) -> Result<T, Error> + Send + Sync + Clone + 'static,
        T: Send + 'static,
    {
        let balancer = self.clone();
        let kind_string = kind.to_string();
        let kind_for_error = kind.to_string();

        match spawn_blocking(move || balancer.call_sync(&kind_string, f)).await {
            Ok(result) => result,
            Err(e) => {
                let context =
                    format!("Failed to spawn blocking task for operation '{kind_for_error}'");
                let error = Error::IOError(std::io::Error::other(e.to_string()));
                Err(MultiError::new(vec![error], context))
            }
        }
    }

    /// Execute the given closure using one of the Electrum clients synchronously.
    ///
    /// This version blocks for client creation if needed but executes the request synchronously.
    /// Used for implementing the ElectrumApi trait.
    ///
    /// If the closure returns an I/O error or certificate error the balancer will try the next
    /// node until all nodes have been exhausted. The last encountered error
    /// is returned in that case.
    ///
    /// Returns `MultiError` containing all individual failures, which can be inspected
    /// by the caller or automatically converted to a single `Error` for compatibility.
    #[instrument(level = "debug", skip(self, f), fields(operation = kind, total_clients = self.client_count(), min_retries = self.config.min_retries))]
    fn call_sync<F, T>(&self, kind: &str, mut f: F) -> Result<T, MultiError>
    where
        F: FnMut(&C) -> Result<T, Error>,
    {
        const INITIAL_BACKOFF: Duration = Duration::from_millis(100);
        const MAX_BACKOFF: Duration = Duration::from_millis(1500);

        let num_clients = self.client_count();
        let mut errors = Vec::new();

        // Try all electrum clients at least once, or min_retries (whichever is higher)
        let allowed_retries = std::cmp::max(self.config.min_retries, num_clients);
        let mut backoff = INITIAL_BACKOFF;

        while errors.len() < allowed_retries {
            // Back off only once a full pass over all servers has failed
            if !errors.is_empty() && errors.len() % num_clients == 0 {
                trace!(
                    backoff_duration_ms = backoff.as_millis(),
                    "All servers failed in this pass, backing off before the next pass"
                );
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }

            // Every pass starts at the primary (first) server
            let idx = errors.len() % num_clients;

            let client = match self.get_or_init_client_sync(idx) {
                Ok(client) => client,
                Err(err) => {
                    trace!(
                        server_url = self.urls[idx],
                        attempt = errors.len(),
                        error = ?err,
                        "Client initialization failed, switching to next client"
                    );

                    errors.push(err);
                    continue;
                }
            };

            let start = Instant::now();
            match f(&client) {
                Ok(res) => {
                    trace!(
                        server_url = self.urls[idx],
                        attempt = errors.len(),
                        duration_ms = start.elapsed().as_millis(),
                        "Electrum operation successful"
                    );
                    return Ok(res);
                }
                Err(err) => {
                    trace!(
                        server_url = self.urls[idx],
                        attempt = errors.len(),
                        duration_ms = start.elapsed().as_millis(),
                        error = ?err,
                        "Electrum operation failed, switching to next client"
                    );

                    errors.push(err);
                }
            }
        }

        warn!(
            operation = kind,
            attempts = errors.len(),
            total_attempts = allowed_retries,
            total_clients = self.client_count(),
            error_count = errors.len(),
            all_errors = ?errors,
            "All Electrum clients failed after exhausting retry attempts with backoff"
        );

        let context = format!(
            "All {} Electrum clients failed after {} attempts for operation '{}'",
            self.client_count(),
            errors.len(),
            kind
        );

        Err(MultiError::new(errors, context))
    }

    /// Execute the given closure on all Electrum nodes in parallel, returning
    /// as soon as `min_parallel_responses` nodes have responded successfully
    /// and the primary (first) node has produced a result.
    ///
    /// The requests to the remaining nodes keep running in the background;
    /// their results are discarded. If the quorum cannot be reached the call
    /// returns once every node has responded.
    ///
    /// The returned results are in completion order, not node order. The
    /// caller is responsible for judging whether the collected responses are
    /// sufficient evidence to act on.
    #[instrument(level = "debug", skip(self, f), fields(operation = kind, total_clients = self.client_count(), min_parallel_responses = self.config.min_parallel_responses))]
    pub async fn join_quorum<F, T>(&self, kind: &str, f: F) -> Result<Vec<Result<T, Error>>, Error>
    where
        F: Fn(&C) -> Result<T, Error> + Send + Sync + Clone + 'static,
        T: Send + 'static,
    {
        let start_time = Instant::now();
        let num_clients = self.client_count();
        let quorum = self.config.min_parallel_responses.clamp(1, num_clients);

        let mut tasks: FuturesUnordered<_> = (0..num_clients)
            .map(|idx| {
                let f = f.clone();
                let balancer = self.clone();

                tokio::spawn(async move {
                    let result = match balancer.get_or_init_client_async(idx).await {
                        Ok(client) => tokio::task::spawn_blocking(move || f(&client))
                            .await
                            .map_err(|e| Error::IOError(std::io::Error::other(format!("{e:?}"))))
                            .and_then(|result| result),
                        Err(e) => Err(e),
                    };

                    (idx, result)
                })
            })
            .collect();

        let mut results: Vec<Result<T, Error>> = Vec::new();
        let mut successes = 0;
        let mut primary_finished = false;

        while let Some(joined) = tasks.next().await {
            match joined {
                Ok((idx, result)) => {
                    if result.is_ok() {
                        successes += 1;
                    }
                    if idx == 0 {
                        primary_finished = true;
                    }
                    results.push(result);
                }
                Err(err) if err.is_cancelled() => {
                    // If one task is cancelled, we do not continue.
                    // Most likely our function got cancelled.
                    return Err(Error::IOError(std::io::Error::other("Task cancelled")));
                }
                Err(err) => {
                    trace!(error = ?err, "Failed to join task for parallel request");
                }
            }

            if successes >= quorum && primary_finished {
                break;
            }
        }

        trace!(
            total_duration_ms = start_time.elapsed().as_millis(),
            successful_requests = successes,
            collected_requests = results.len(),
            detached_requests = tasks.len(),
            "Quorum execution completed, remaining requests continue in the background"
        );

        Ok(results)
    }

    /// Broadcast the given transaction to all Electrum nodes in parallel.
    ///
    /// The transaction is sent to every node, but the method already returns
    /// once `min_parallel_responses` nodes have accepted it and the primary
    /// node has responded. The remaining broadcasts continue in the background.
    /// Errors for individual nodes do not abort the others.
    #[instrument(level = "debug", skip(self, tx), fields(txid = %tx.compute_txid(), total_clients = self.client_count()))]
    pub async fn broadcast_all(
        &self,
        tx: Transaction,
    ) -> Result<Vec<Result<bitcoin::Txid, Error>>, Error> {
        let txid = tx.compute_txid();
        let start_time = Instant::now();

        debug!(
            txid = %txid,
            total_clients = self.client_count(),
            "Broadcasting transaction to electrum clients"
        );

        let results = self
            .join_quorum("transaction_broadcast", move |client| {
                client.transaction_broadcast(&tx)
            })
            .await?;

        let success_count = results.iter().filter(|r| r.is_ok()).count();

        if success_count > 0 {
            debug!(
                txid = %txid,
                successful_broadcasts = success_count,
                total_attempts = results.len(),
                duration_ms = start_time.elapsed().as_millis(),
                "Transaction broadcast completed successfully"
            );
        } else {
            error!(
                txid = %txid,
                total_attempts = results.len(),
                duration_ms = start_time.elapsed().as_millis(),
                "Transaction broadcast failed on all servers"
            );
        }

        Ok(results)
    }

    /// Get the URLs used by this balancer
    pub fn urls(&self) -> &Vec<String> {
        &self.urls
    }

    /// Get the current configuration
    pub fn config(&self) -> &ElectrumBalancerConfig {
        &self.config
    }

    /// Populate the transaction cache for all initialized clients.
    pub fn populate_tx_cache(&self, txs: impl IntoIterator<Item = impl Into<Arc<Transaction>>>) {
        // Convert transactions to Arc<Transaction> and collect them since we'll use them for each client
        let transactions: Vec<Arc<Transaction>> = txs.into_iter().map(|tx| tx.into()).collect();
        let clients = self.clients.read().expect("rwlock poisoned");

        let mut initialized_count = 0;

        // Only populate cache for already initialized clients
        for client_once_cell in clients.iter() {
            if let Some(client) = client_once_cell.get() {
                client.populate_tx_cache(transactions.iter().cloned());
                initialized_count += 1;
            }
        }

        trace!(
            transaction_count = transactions.len(),
            initialized_client_count = initialized_count,
            total_client_count = clients.len(),
            "Populated transaction cache for initialized clients"
        );
    }
}

impl<C> Clone for ElectrumBalancer<C>
where
    C: ElectrumClientLike,
{
    fn clone(&self) -> Self {
        Self {
            urls: self.urls.clone(),
            clients: self.clients.clone(),
            config: self.config.clone(),
            factory: self.factory.clone(),
        }
    }
}

/// Trait abstracting Electrum client operations needed by the balancer
pub trait ElectrumClientLike: Send + Sync + 'static {
    /// Broadcast a transaction
    fn transaction_broadcast(&self, tx: &Transaction) -> Result<bitcoin::Txid, Error>;

    /// Populate transaction cache (only for BdkElectrumClient)
    fn populate_tx_cache(&self, _txs: impl Iterator<Item = Arc<Transaction>>) {
        // Default implementation does nothing
    }
}

impl ElectrumClientLike for BdkElectrumClient<Client> {
    fn transaction_broadcast(&self, tx: &Transaction) -> Result<bitcoin::Txid, Error> {
        self.inner.transaction_broadcast(tx)
    }

    fn populate_tx_cache(&self, txs: impl Iterator<Item = Arc<Transaction>>) {
        BdkElectrumClient::populate_tx_cache(self, txs)
    }
}

/// Configuration for the Electrum balancer
#[derive(Clone, Debug)]
pub struct ElectrumBalancerConfig {
    /// Timeout for individual requests in seconds
    pub request_timeout: u8,
    /// Minimum number of retry attempts across all nodes
    pub min_retries: usize,
    /// Number of successful responses parallel multi-node operations
    /// ([`ElectrumBalancer::join_quorum`], [`ElectrumBalancer::broadcast_all`])
    /// wait for before returning early. Clamped to the number of nodes.
    pub min_parallel_responses: usize,
}

impl Default for ElectrumBalancerConfig {
    fn default() -> Self {
        Self {
            request_timeout: 15,
            min_retries: 5,
            min_parallel_responses: 2,
        }
    }
}

/// Trait for creating Electrum clients
pub trait ElectrumClientFactory<C> {
    fn create_client(&self, url: &str, config: &ElectrumBalancerConfig) -> Result<Arc<C>, Error>;
}

/// Default factory for BdkElectrumClient
pub struct BdkElectrumClientFactory;

impl ElectrumClientFactory<BdkElectrumClient<Client>> for BdkElectrumClientFactory {
    fn create_client(
        &self,
        url: &str,
        config: &ElectrumBalancerConfig,
    ) -> Result<Arc<BdkElectrumClient<Client>>, Error> {
        let client_config = ConfigBuilder::new()
            .timeout(Some(config.request_timeout))
            // TODO: Why is this set to 1?
            // The goal of this crate is to extract retry logic out of the electrum client library
            // and instead handle inside this crate. However, the electrum client library is quite inflexible.
            //
            // Setting it to 0 causes some bugs, see: https://github.com/bitcoindevkit/rust-electrum-client/issues/186
            .retry(1)
            .build();

        let client = Client::from_config(url, client_config).map_err(|e| {
            // Wrap connection errors with DNS resolution context
            match &e {
                Error::IOError(io_err) if io_err.kind() == std::io::ErrorKind::NotFound => {
                    Error::IOError(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("{e} (Most likely DNS resolution error)"),
                    ))
                }
                Error::IOError(io_err)
                    if io_err.kind() == std::io::ErrorKind::TimedOut
                        || io_err.kind() == std::io::ErrorKind::ConnectionRefused
                        || io_err.kind() == std::io::ErrorKind::ConnectionAborted
                        || io_err.kind() == std::io::ErrorKind::Other =>
                {
                    Error::IOError(std::io::Error::new(
                        io_err.kind(),
                        format!("{e} (Most likely DNS resolution error)"),
                    ))
                }
                _ => e, // Pass through other errors unchanged
            }
        })?;
        let bdk_client = BdkElectrumClient::new(client);

        Ok(Arc::new(bdk_client))
    }
}

// Convenience methods for the default BdkElectrumClient case
impl ElectrumBalancer<BdkElectrumClient<Client>> {
    /// Create a new balancer from a list of Electrum URLs with default configuration.
    /// Uses the default BdkElectrumClientFactory.
    pub async fn new(urls: Vec<String>) -> Result<Self, Error> {
        Self::new_with_factory(urls, Arc::new(BdkElectrumClientFactory)).await
    }

    /// Create a new balancer from a list of Electrum URLs with custom configuration.
    /// Uses the default BdkElectrumClientFactory.
    pub async fn new_with_config(
        urls: Vec<String>,
        config: ElectrumBalancerConfig,
    ) -> Result<Self, Error> {
        Self::new_with_config_and_factory(urls, config, Arc::new(BdkElectrumClientFactory)).await
    }
}

/// Type alias for the default Electrum balancer using BdkElectrumClient
pub type DefaultElectrumBalancer = ElectrumBalancer<BdkElectrumClient<Client>>;

/// Error type that contains multiple Electrum errors from different nodes.
///
/// This allows the caller to inspect all individual failures while still
/// working with the `?` operator through automatic conversion to a single Error.
#[derive(Debug)]
pub struct MultiError {
    pub errors: Vec<Error>,
    pub context: String,
}

impl Clone for MultiError {
    fn clone(&self) -> Self {
        // Clone by converting each error to a string and back to an error
        let cloned_errors = self
            .errors
            .iter()
            .map(|e| Error::IOError(std::io::Error::other(e.to_string())))
            .collect();

        Self {
            errors: cloned_errors,
            context: self.context.clone(),
        }
    }
}

impl MultiError {
    pub fn new(errors: Vec<Error>, context: impl Into<String>) -> Self {
        Self {
            errors,
            context: context.into(),
        }
    }

    /// Get the number of errors
    pub fn len(&self) -> usize {
        self.errors.len()
    }

    /// Check if there are no errors
    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    /// Get an iterator over the errors
    pub fn iter(&self) -> impl Iterator<Item = &Error> {
        self.errors.iter()
    }

    /// Check if any error matches a predicate
    pub fn any<F>(&self, predicate: F) -> bool
    where
        F: Fn(&Error) -> bool,
    {
        self.errors.iter().any(predicate)
    }

    /// Check if all errors match a predicate
    pub fn all<F>(&self, predicate: F) -> bool
    where
        F: Fn(&Error) -> bool,
    {
        self.errors.iter().all(predicate)
    }

    /// Convert to a single Error (uses the last error, or creates a generic one)
    pub fn into_single_error(self) -> Error {
        self.errors.into_iter().next_back().unwrap_or_else(|| {
            Error::IOError(std::io::Error::other(format!(
                "All operations failed: {}",
                self.context
            )))
        })
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

impl std::error::Error for MultiError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // Return the last error as the source
        self.errors.last().and_then(|e| e.source())
    }
}

impl From<MultiError> for Error {
    fn from(multi_error: MultiError) -> Self {
        multi_error.into_single_error()
    }
}

// Allow ? operator to work on MultiError by converting to Error
impl<T> From<MultiError> for Result<T, Error> {
    fn from(multi_error: MultiError) -> Self {
        Err(multi_error.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::hashes::Hash;
    use bitcoin::{
        Amount, OutPoint, ScriptBuf, Sequence, TxIn, TxOut, Witness, absolute::LockTime,
        transaction::Version,
    };
    use std::sync::Mutex as StdMutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Mock client for testing
    #[derive(Debug)]
    struct MockElectrumClient {
        url: String,
        fail_count: Arc<AtomicUsize>,
        call_count: Arc<AtomicUsize>,
        should_fail: bool,
        error_type: MockErrorType,
        delay: Option<Duration>,
    }

    #[derive(Debug, Clone)]
    enum MockErrorType {
        IOError,
        NonRetryable,
    }

    impl MockElectrumClient {
        fn new(url: String) -> Self {
            Self {
                url,
                fail_count: Arc::new(AtomicUsize::new(0)),
                call_count: Arc::new(AtomicUsize::new(0)),
                should_fail: false,
                error_type: MockErrorType::IOError,
                delay: None,
            }
        }

        fn with_failure(mut self, error_type: MockErrorType) -> Self {
            self.should_fail = true;
            self.error_type = error_type;
            self
        }

        fn with_delay(mut self, delay: Duration) -> Self {
            self.delay = Some(delay);
            self
        }

        fn call_count(&self) -> usize {
            self.call_count.load(Ordering::SeqCst)
        }
    }

    impl ElectrumClientLike for MockElectrumClient {
        fn transaction_broadcast(&self, _tx: &Transaction) -> Result<bitcoin::Txid, Error> {
            self.call_count.fetch_add(1, Ordering::SeqCst);

            if let Some(delay) = self.delay {
                std::thread::sleep(delay);
            }

            if self.should_fail {
                self.fail_count.fetch_add(1, Ordering::SeqCst);
                match self.error_type {
                    MockErrorType::IOError => Err(Error::IOError(std::io::Error::new(
                        std::io::ErrorKind::ConnectionRefused,
                        format!("Mock connection failed for {}", self.url),
                    ))),
                    MockErrorType::NonRetryable => Err(Error::Protocol(
                        format!(
                            "\"code\": Number(-5) - transaction not found on {}",
                            self.url
                        )
                        .into(),
                    )),
                }
            } else {
                Ok(bitcoin::Txid::from_raw_hash(
                    bitcoin::hashes::sha256d::Hash::from_byte_array([1; 32]),
                ))
            }
        }
    }

    /// Mock factory for creating test clients
    struct MockElectrumClientFactory {
        clients: Arc<StdMutex<Vec<Arc<MockElectrumClient>>>>,
    }

    impl MockElectrumClientFactory {
        fn new() -> Self {
            Self {
                clients: Arc::new(StdMutex::new(Vec::new())),
            }
        }

        fn add_client(&self, client: MockElectrumClient) {
            self.clients.lock().unwrap().push(Arc::new(client));
        }

        fn get_client(&self, idx: usize) -> Option<Arc<MockElectrumClient>> {
            self.clients.lock().unwrap().get(idx).cloned()
        }
    }

    impl ElectrumClientFactory<MockElectrumClient> for MockElectrumClientFactory {
        fn create_client(
            &self,
            url: &str,
            _config: &ElectrumBalancerConfig,
        ) -> Result<Arc<MockElectrumClient>, Error> {
            let clients = self.clients.lock().unwrap();
            for client in clients.iter() {
                if client.url == url {
                    return Ok(client.clone());
                }
            }

            // If no pre-configured client found, create a default one
            Ok(Arc::new(MockElectrumClient::new(url.to_string())))
        }
    }

    fn create_dummy_transaction() -> Transaction {
        Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint::null(),
                script_sig: ScriptBuf::new(),
                sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                witness: Witness::new(),
            }],
            output: vec![TxOut {
                value: Amount::from_sat(1000),
                script_pubkey: ScriptBuf::new(),
            }],
        }
    }

    #[tokio::test]
    async fn test_balancer_creation() {
        let urls = vec![
            "tcp://localhost:50001".to_string(),
            "tcp://localhost:50002".to_string(),
        ];

        let factory = Arc::new(MockElectrumClientFactory::new());
        let balancer = ElectrumBalancer::new_with_factory(urls.clone(), factory).await;

        assert!(balancer.is_ok());
        let balancer = balancer.unwrap();
        assert_eq!(balancer.client_count(), 2);
        assert_eq!(balancer.urls(), &urls);
    }

    #[tokio::test]
    async fn test_balancer_empty_urls() {
        let factory = Arc::new(MockElectrumClientFactory::new());
        let balancer = ElectrumBalancer::new_with_factory(vec![], factory).await;

        assert!(balancer.is_err());
        match balancer {
            Err(e) => assert!(e.to_string().contains("No Electrum URLs provided")),
            Ok(_) => panic!("Expected error but got Ok"),
        }
    }

    #[tokio::test]
    async fn test_call_always_starts_with_primary() {
        let urls = vec![
            "tcp://localhost:50001".to_string(),
            "tcp://localhost:50002".to_string(),
            "tcp://localhost:50003".to_string(),
        ];

        let factory = Arc::new(MockElectrumClientFactory::new());
        for url in &urls {
            factory.add_client(MockElectrumClient::new(url.clone()));
        }

        let balancer = ElectrumBalancer::new_with_factory(urls, factory.clone())
            .await
            .unwrap();

        // Make several successful calls and verify they all hit the primary
        for _ in 0..6 {
            let result = balancer
                .call("test", |client| {
                    client.transaction_broadcast(&create_dummy_transaction())
                })
                .await;

            assert!(result.is_ok());
        }

        // Verify only the first client was used
        assert_eq!(factory.get_client(0).unwrap().call_count(), 6);
        assert_eq!(factory.get_client(1).unwrap().call_count(), 0);
        assert_eq!(factory.get_client(2).unwrap().call_count(), 0);
    }

    #[tokio::test]
    async fn test_call_switches_on_failure() {
        let urls = vec![
            "tcp://localhost:50001".to_string(),
            "tcp://localhost:50002".to_string(),
            "tcp://localhost:50003".to_string(),
        ];

        let factory = Arc::new(MockElectrumClientFactory::new());
        // First client fails, second succeeds, third not used
        factory.add_client(
            MockElectrumClient::new(urls[0].clone()).with_failure(MockErrorType::IOError),
        );
        factory.add_client(MockElectrumClient::new(urls[1].clone()));
        factory.add_client(MockElectrumClient::new(urls[2].clone()));

        // Use config with min_retries = 0 to test basic switching behavior
        // This ensures total_attempts = max(0, 3) = 3, but behavior is cleaner
        let config = ElectrumBalancerConfig {
            request_timeout: 5,
            min_retries: 0,
            min_parallel_responses: 2,
        };

        let balancer = ElectrumBalancer::new_with_config_and_factory(urls, config, factory.clone())
            .await
            .unwrap();

        // First call should try client 0 (fails), then client 1 (succeeds)
        let result1 = balancer
            .call("test", |client| {
                client.transaction_broadcast(&create_dummy_transaction())
            })
            .await;
        assert!(result1.is_ok());

        // The second call starts at the primary again
        let result2 = balancer
            .call("test", |client| {
                client.transaction_broadcast(&create_dummy_transaction())
            })
            .await;
        assert!(result2.is_ok());

        assert_eq!(factory.get_client(0).unwrap().call_count(), 2); // Tried first on both calls
        assert_eq!(factory.get_client(1).unwrap().call_count(), 2); // Used by both calls after client 0 fails
        assert_eq!(factory.get_client(2).unwrap().call_count(), 0); // Never called
    }

    #[tokio::test]
    async fn test_call_with_failing_client() {
        let urls = vec![
            "tcp://localhost:50001".to_string(),
            "tcp://localhost:50002".to_string(),
        ];

        let factory = Arc::new(MockElectrumClientFactory::new());
        // First client fails, second succeeds
        factory.add_client(
            MockElectrumClient::new(urls[0].clone()).with_failure(MockErrorType::IOError),
        );
        factory.add_client(MockElectrumClient::new(urls[1].clone()));

        let balancer = ElectrumBalancer::new_with_factory(urls, factory.clone())
            .await
            .unwrap();

        let result = balancer
            .call("test", |client| {
                client.transaction_broadcast(&create_dummy_transaction())
            })
            .await;

        assert!(result.is_ok());

        // Verify the failing client was called once and the successful client was called once
        assert_eq!(factory.get_client(0).unwrap().call_count(), 1);
        assert_eq!(factory.get_client(1).unwrap().call_count(), 1);
    }

    #[tokio::test]
    async fn test_call_with_non_retryable_error() {
        let urls = vec!["tcp://localhost:50001".to_string()];

        let factory = Arc::new(MockElectrumClientFactory::new());
        factory.add_client(
            MockElectrumClient::new(urls[0].clone()).with_failure(MockErrorType::NonRetryable),
        );

        // min_retries = 1 limits the call to a single attempt
        let config = ElectrumBalancerConfig {
            request_timeout: 5,
            min_retries: 1,
            min_parallel_responses: 2,
        };

        let balancer = ElectrumBalancer::new_with_config_and_factory(urls, config, factory.clone())
            .await
            .unwrap();

        let result = balancer
            .call("test", |client| {
                client.transaction_broadcast(&create_dummy_transaction())
            })
            .await;

        assert!(result.is_err());
        match result {
            Err(e) => assert!(e.to_string().contains("transaction not found")),
            Ok(_) => panic!("Expected error but got Ok"),
        }

        // Should only be called once (no retry for non-retryable errors)
        assert_eq!(factory.get_client(0).unwrap().call_count(), 1);
    }

    #[tokio::test]
    async fn test_call_all_clients_fail() {
        let urls = vec![
            "tcp://localhost:50001".to_string(),
            "tcp://localhost:50002".to_string(),
        ];

        let factory = Arc::new(MockElectrumClientFactory::new());
        factory.add_client(
            MockElectrumClient::new(urls[0].clone()).with_failure(MockErrorType::IOError),
        );
        factory.add_client(
            MockElectrumClient::new(urls[1].clone()).with_failure(MockErrorType::IOError),
        );

        let balancer = ElectrumBalancer::new_with_factory(urls, factory.clone())
            .await
            .unwrap();

        let result = balancer
            .call("test", |client| {
                client.transaction_broadcast(&create_dummy_transaction())
            })
            .await;

        assert!(result.is_err());
        match result {
            Err(e) => {
                let error_msg = e.to_string();
                println!("Error message: {error_msg}");
                assert!(
                    error_msg.contains("All Electrum nodes failed")
                        || error_msg.contains("Mock connection failed")
                );
            }
            Ok(_) => panic!("Expected error but got Ok"),
        }

        // Both clients should have been tried multiple times due to min_retries
        assert!(factory.get_client(0).unwrap().call_count() > 1);
        assert!(factory.get_client(1).unwrap().call_count() > 1);
    }

    #[tokio::test]
    async fn test_broadcast_all() {
        let urls = vec![
            "tcp://localhost:50001".to_string(),
            "tcp://localhost:50002".to_string(),
        ];

        let factory = Arc::new(MockElectrumClientFactory::new());
        factory.add_client(MockElectrumClient::new(urls[0].clone()));
        factory.add_client(MockElectrumClient::new(urls[1].clone()));

        let balancer = ElectrumBalancer::new_with_factory(urls, factory.clone())
            .await
            .unwrap();

        let tx = create_dummy_transaction();
        let results = balancer.broadcast_all(tx).await;

        assert!(results.is_ok());
        let results = results.unwrap();
        assert_eq!(results.len(), 2);

        // Both should succeed
        assert!(results[0].is_ok());
        assert!(results[1].is_ok());

        // Both clients should have been called
        assert_eq!(factory.get_client(0).unwrap().call_count(), 1);
        assert_eq!(factory.get_client(1).unwrap().call_count(), 1);
    }

    #[tokio::test]
    async fn test_config_and_urls_accessors() {
        let urls = vec!["tcp://localhost:50001".to_string()];
        let config = ElectrumBalancerConfig {
            request_timeout: 15,
            min_retries: 7,
            min_parallel_responses: 3,
        };

        let factory = Arc::new(MockElectrumClientFactory::new());
        let balancer =
            ElectrumBalancer::new_with_config_and_factory(urls.clone(), config.clone(), factory)
                .await
                .unwrap();

        assert_eq!(balancer.urls(), &urls);
        assert_eq!(balancer.config().request_timeout, 15);
        assert_eq!(balancer.config().min_retries, 7);
    }

    #[tokio::test]
    async fn test_populate_tx_cache() {
        let urls = vec!["tcp://localhost:50001".to_string()];

        let factory = Arc::new(MockElectrumClientFactory::new());
        factory.add_client(MockElectrumClient::new(urls[0].clone()));

        let balancer = ElectrumBalancer::new_with_factory(urls, factory.clone())
            .await
            .unwrap();

        // Initialize the client first
        let _ = balancer.call("test", |client| Ok(client.url.clone())).await;

        // This should not panic (MockElectrumClient has default implementation)
        let txs = vec![create_dummy_transaction()];
        balancer.populate_tx_cache(txs);
    }

    #[tokio::test]
    async fn test_multi_error_functionality() {
        let urls = vec![
            "tcp://localhost:50001".to_string(),
            "tcp://localhost:50002".to_string(),
            "tcp://localhost:50003".to_string(),
        ];

        let factory = Arc::new(MockElectrumClientFactory::new());
        factory.add_client(
            MockElectrumClient::new(urls[0].clone()).with_failure(MockErrorType::IOError),
        );
        factory.add_client(
            MockElectrumClient::new(urls[1].clone()).with_failure(MockErrorType::NonRetryable),
        );
        factory.add_client(
            MockElectrumClient::new(urls[2].clone()).with_failure(MockErrorType::IOError),
        );

        let balancer = ElectrumBalancer::new_with_factory(urls, factory.clone())
            .await
            .unwrap();

        // Use call_async_with_multi_error to get the MultiError
        let result = balancer
            .call_async_with_multi_error("test", |client| {
                client.transaction_broadcast(&create_dummy_transaction())
            })
            .await;

        assert!(result.is_err());
        let multi_error = result.unwrap_err();

        // Check that we have multiple errors
        assert!(multi_error.len() > 1);
        assert!(!multi_error.is_empty());

        // Check that we can inspect individual errors
        let error_count = multi_error.errors.len();
        assert!(error_count > 0);

        // Test the `any` method to find specific error types
        let has_non_retryable =
            multi_error.any(|e| e.to_string().contains("transaction not found"));
        assert!(has_non_retryable);

        // Test converting to single error (should work with ?)
        let single_error: Error = multi_error.clone().into();
        assert!(!single_error.to_string().is_empty());

        // Test that the ? operator works
        fn test_question_mark(multi_error: MultiError) -> Result<(), Error> {
            Err(multi_error)?
        }

        let result = test_question_mark(multi_error);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_call_async_with_multi_error() {
        let urls = vec![
            "tcp://localhost:50001".to_string(),
            "tcp://localhost:50002".to_string(),
        ];

        let factory = Arc::new(MockElectrumClientFactory::new());
        factory.add_client(
            MockElectrumClient::new(urls[0].clone()).with_failure(MockErrorType::NonRetryable),
        );
        factory.add_client(
            MockElectrumClient::new(urls[1].clone()).with_failure(MockErrorType::IOError),
        );

        let balancer = ElectrumBalancer::new_with_factory(urls, factory.clone())
            .await
            .unwrap();

        let result = balancer
            .call_async_with_multi_error("test", |client| {
                client.transaction_broadcast(&create_dummy_transaction())
            })
            .await;

        assert!(result.is_err());
        let multi_error = result.unwrap_err();

        // Should have multiple errors due to retries (protocol errors fail
        // over to the next server just like I/O errors)
        assert!(multi_error.len() > 2);

        // Check that there are "transaction not found" type errors
        let has_not_found = multi_error.any(|e| e.to_string().contains("transaction not found"));
        assert!(has_not_found);

        // And I/O errors
        let has_io_error = multi_error.any(|e| e.to_string().contains("Mock connection failed"));
        assert!(has_io_error);
    }

    #[tokio::test]
    async fn test_join_quorum_returns_early_with_slow_node() {
        let urls = vec![
            "tcp://localhost:50001".to_string(),
            "tcp://localhost:50002".to_string(),
            "tcp://localhost:50003".to_string(),
        ];

        let factory = Arc::new(MockElectrumClientFactory::new());
        factory.add_client(MockElectrumClient::new(urls[0].clone()));
        factory.add_client(MockElectrumClient::new(urls[1].clone()));
        factory
            .add_client(MockElectrumClient::new(urls[2].clone()).with_delay(Duration::from_secs(2)));

        let config = ElectrumBalancerConfig {
            request_timeout: 5,
            min_retries: 0,
            min_parallel_responses: 2,
        };

        let balancer = ElectrumBalancer::new_with_config_and_factory(urls, config, factory.clone())
            .await
            .unwrap();

        let start = Instant::now();
        let results = balancer
            .join_quorum("transaction_broadcast", |client| {
                client.transaction_broadcast(&create_dummy_transaction())
            })
            .await
            .unwrap();

        // Two fast successes satisfy the quorum, the slow node is left behind
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.is_ok()));
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[tokio::test]
    async fn test_join_quorum_waits_for_primary() {
        let urls = vec![
            "tcp://localhost:50001".to_string(),
            "tcp://localhost:50002".to_string(),
            "tcp://localhost:50003".to_string(),
        ];

        let factory = Arc::new(MockElectrumClientFactory::new());
        factory.add_client(
            MockElectrumClient::new(urls[0].clone()).with_delay(Duration::from_millis(500)),
        );
        factory.add_client(MockElectrumClient::new(urls[1].clone()));
        factory.add_client(MockElectrumClient::new(urls[2].clone()));

        let config = ElectrumBalancerConfig {
            request_timeout: 5,
            min_retries: 0,
            min_parallel_responses: 2,
        };

        let balancer = ElectrumBalancer::new_with_config_and_factory(urls, config, factory.clone())
            .await
            .unwrap();

        let start = Instant::now();
        let results = balancer
            .join_quorum("transaction_broadcast", |client| {
                client.transaction_broadcast(&create_dummy_transaction())
            })
            .await
            .unwrap();

        // The quorum of 2 is reached by the fast nodes, but we still wait
        // for the primary (first) node before returning
        assert_eq!(results.len(), 3);
        assert!(start.elapsed() >= Duration::from_millis(500));
    }
}
