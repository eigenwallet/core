//! A failover [`HttpTransport`] that delegates to a set of inner transports
//! (typically one per daemon), trying them in turn until one succeeds.
//!
//! Unlike a hedging pool, requests are issued sequentially: at most one inner
//! transport handles a request at a time. The pool sits at the `post` layer so
//! it is route-agnostic and covers every daemon endpoint uniformly.
//!
//! Each attempt is sorted into one of three outcomes:
//! - **transport failure** — the inner `post` returned an error (connection,
//!   timeout, `ChannelClosed`), or the body was unusable (a JSON-RPC `error`,
//!   an unparseable body, a non-`OK` `status` other than a rejection). The node
//!   is marked exhausted and the request fails over to the next node.
//! - **protocol failure** — the node processed the request and rejected it on
//!   application grounds (monerod replied with `status: "Failed"`, e.g. "tx is
//!   invalid"). Failing over rarely helps, so this is retried at most
//!   [`MAX_PROTOCOL_ERRORS`] times before the rejection body is surfaced.
//! - **success** — the body is returned to the caller.

use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use monero_daemon_rpc::prelude::InterfaceError;
use monero_daemon_rpc::HttpTransport;

/// How many times a request may be rejected on protocol grounds before the pool
/// stops failing over and surfaces the rejection.
pub const MAX_PROTOCOL_ERRORS: usize = 3;

/// The outcome of inspecting a daemon response body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResponseClass {
    /// A usable response; return it to the caller.
    Success,
    /// The node could not service the request. Fail over to the next node.
    Transport,
    /// The node processed the request and rejected it on application grounds.
    Protocol,
}

/// Classifies a daemon response body. Abstracted so the pool can be unit-tested
/// without real monerod responses, and so the body semantics can be swapped.
pub trait ResponseClassifier: Send + Sync {
    fn classify(&self, route: &str, body: &[u8]) -> ResponseClass;
}

/// The default classifier, aware of monerod's response shapes.
///
/// monerod's non-JSON-RPC endpoints (`send_raw_transaction`, `get_height`, ...)
/// answer with a flat `{ "status": "OK" | "Failed" | ... }` object, while
/// `/json_rpc` methods nest `status` under `result` or report an `error`.
pub struct MoneroStatusClassifier;

impl ResponseClassifier for MoneroStatusClassifier {
    fn classify(&self, _route: &str, body: &[u8]) -> ResponseClass {
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
            return ResponseClass::Transport;
        };

        // A JSON-RPC error from the node (and our own validity sanity checks)
        // is a transport-class failure, not an application rejection.
        if value.get("error").is_some() {
            return ResponseClass::Transport;
        }

        let status = value
            .get("status")
            .or_else(|| value.get("result").and_then(|result| result.get("status")))
            .and_then(serde_json::Value::as_str);

        match status {
            None | Some("OK") => ResponseClass::Success,
            Some("Failed") => ResponseClass::Protocol,
            Some(_) => ResponseClass::Transport,
        }
    }
}

/// An [`HttpTransport`] that fails over across a set of inner transports.
///
/// A request starts at the most-recently-successful transport and walks the
/// remaining transports in order, so a healthy node is preferred without
/// re-probing dead nodes on every call.
#[derive(Clone)]
pub struct FailoverTransport<T> {
    transports: Arc<Vec<T>>,
    classifier: Arc<dyn ResponseClassifier>,
    preferred: Arc<AtomicUsize>,
}

impl<T> FailoverTransport<T> {
    /// Build a failover transport over the given inner transports, using the
    /// monerod-aware [`MoneroStatusClassifier`].
    ///
    /// Returns `None` if `transports` is empty, since a pool with no nodes could
    /// never fulfil a request.
    pub fn new(transports: Vec<T>) -> Option<Self> {
        Self::with_classifier(transports, Arc::new(MoneroStatusClassifier))
    }

    /// Build a failover transport with a custom response classifier.
    pub fn with_classifier(
        transports: Vec<T>,
        classifier: Arc<dyn ResponseClassifier>,
    ) -> Option<Self> {
        if transports.is_empty() {
            return None;
        }

        Some(Self {
            transports: Arc::new(transports),
            classifier,
            preferred: Arc::new(AtomicUsize::new(0)),
        })
    }

    /// The number of inner transports.
    pub fn len(&self) -> usize {
        self.transports.len()
    }

    pub fn is_empty(&self) -> bool {
        self.transports.is_empty()
    }
}

impl<T: HttpTransport + Send> HttpTransport for FailoverTransport<T> {
    fn post(
        &self,
        route: &str,
        body: Vec<u8>,
        response_size_limit: Option<usize>,
    ) -> impl Send + Future<Output = Result<Vec<u8>, InterfaceError>> {
        async move {
            let node_count = self.transports.len();
            let mut exhausted = vec![false; node_count];
            let mut protocol_errors = 0usize;

            // The most recent unaccepted outcome, surfaced once we run out of
            // options so the caller sees a real response rather than a synthetic
            // error.
            let mut last: Option<Result<Vec<u8>, InterfaceError>> = None;

            let mut index = self.preferred.load(Ordering::Relaxed) % node_count;

            while !exhausted.iter().all(|&node| node) {
                while exhausted[index] {
                    index = (index + 1) % node_count;
                }

                match self.transports[index]
                    .post(route, body.clone(), response_size_limit)
                    .await
                {
                    Err(error) => {
                        tracing::warn!(node = index, %route, %error, "Monero RPC node transport error; failing over");
                        last = Some(Err(error));
                        exhausted[index] = true;
                        index = (index + 1) % node_count;
                    }
                    Ok(bytes) => match self.classifier.classify(route, &bytes) {
                        ResponseClass::Success => {
                            self.preferred.store(index, Ordering::Relaxed);
                            return Ok(bytes);
                        }
                        ResponseClass::Transport => {
                            tracing::warn!(node = index, %route, "Monero RPC node returned an unusable response; failing over");
                            last = Some(Ok(bytes));
                            exhausted[index] = true;
                            index = (index + 1) % node_count;
                        }
                        ResponseClass::Protocol => {
                            protocol_errors += 1;
                            tracing::warn!(node = index, %route, protocol_errors, "Monero RPC node rejected the request");
                            if protocol_errors >= MAX_PROTOCOL_ERRORS {
                                return Ok(bytes);
                            }
                            last = Some(Ok(bytes));
                            index = (index + 1) % node_count;
                        }
                    },
                }
            }

            last.unwrap_or_else(|| {
                Err(InterfaceError::InterfaceError(
                    "failover transport exhausted all nodes".to_owned(),
                ))
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A scripted response from a mock node, consumed per call (saturating at the
    /// last entry so repeated calls keep returning it).
    #[derive(Clone)]
    enum Reply {
        /// The inner `post` succeeds with this body.
        Body(&'static str),
        /// The inner `post` fails at the transport layer.
        TransportError,
    }

    #[derive(Clone)]
    struct MockNode {
        id: usize,
        log: Arc<Mutex<Vec<usize>>>,
        replies: Arc<Vec<Reply>>,
        calls: Arc<AtomicUsize>,
    }

    impl MockNode {
        fn new(id: usize, log: Arc<Mutex<Vec<usize>>>, replies: Vec<Reply>) -> Self {
            Self {
                id,
                log,
                replies: Arc::new(replies),
                calls: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    impl HttpTransport for MockNode {
        fn post(
            &self,
            _route: &str,
            _body: Vec<u8>,
            _response_size_limit: Option<usize>,
        ) -> impl Send + Future<Output = Result<Vec<u8>, InterfaceError>> {
            let id = self.id;
            let log = self.log.clone();
            let replies = self.replies.clone();
            let calls = self.calls.clone();
            async move {
                log.lock().unwrap().push(id);
                let index = calls.fetch_add(1, Ordering::SeqCst).min(replies.len() - 1);
                match &replies[index] {
                    Reply::Body(body) => Ok(body.as_bytes().to_vec()),
                    Reply::TransportError => {
                        Err(InterfaceError::InterfaceError(format!("node {id} unreachable")))
                    }
                }
            }
        }
    }

    /// Interprets a mock body as a class marker, decoupling pool tests from the
    /// JSON parsing in [`MoneroStatusClassifier`].
    struct MarkerClassifier;

    impl ResponseClassifier for MarkerClassifier {
        fn classify(&self, _route: &str, body: &[u8]) -> ResponseClass {
            match body {
                b"ok" => ResponseClass::Success,
                b"protocol" => ResponseClass::Protocol,
                _ => ResponseClass::Transport,
            }
        }
    }

    fn pool(replies_per_node: Vec<Vec<Reply>>) -> (FailoverTransport<MockNode>, Arc<Mutex<Vec<usize>>>) {
        let log = Arc::new(Mutex::new(Vec::new()));
        let nodes = replies_per_node
            .into_iter()
            .enumerate()
            .map(|(id, replies)| MockNode::new(id, log.clone(), replies))
            .collect();
        let transport =
            FailoverTransport::with_classifier(nodes, Arc::new(MarkerClassifier)).unwrap();
        (transport, log)
    }

    async fn post(transport: &FailoverTransport<MockNode>) -> Result<Vec<u8>, InterfaceError> {
        transport.post("get_height", vec![], None).await
    }

    #[tokio::test]
    async fn success_returns_first_node_without_touching_others() {
        let (transport, log) = pool(vec![vec![Reply::Body("ok")], vec![Reply::Body("ok")]]);
        assert_eq!(post(&transport).await.unwrap(), b"ok");
        assert_eq!(*log.lock().unwrap(), vec![0]);
    }

    #[tokio::test]
    async fn transport_error_fails_over_to_next_node() {
        let (transport, log) = pool(vec![vec![Reply::TransportError], vec![Reply::Body("ok")]]);
        assert_eq!(post(&transport).await.unwrap(), b"ok");
        assert_eq!(*log.lock().unwrap(), vec![0, 1]);
    }

    #[tokio::test]
    async fn transport_errors_exhaust_every_node() {
        let (transport, log) = pool(vec![
            vec![Reply::TransportError],
            vec![Reply::TransportError],
            vec![Reply::TransportError],
        ]);
        assert!(post(&transport).await.is_err());
        assert_eq!(*log.lock().unwrap(), vec![0, 1, 2]);
    }

    #[tokio::test]
    async fn unusable_body_is_a_transport_failure_and_fails_over() {
        let (transport, log) = pool(vec![vec![Reply::Body("busy")], vec![Reply::Body("ok")]]);
        assert_eq!(post(&transport).await.unwrap(), b"ok");
        assert_eq!(*log.lock().unwrap(), vec![0, 1]);
    }

    #[tokio::test]
    async fn unusable_bodies_exhaust_and_surface_the_last_body() {
        let (transport, log) = pool(vec![vec![Reply::Body("busy")], vec![Reply::Body("busy")]]);
        // Last unaccepted outcome is a body, surfaced so the caller can parse the
        // real error rather than a synthetic one.
        assert_eq!(post(&transport).await.unwrap(), b"busy");
        assert_eq!(*log.lock().unwrap(), vec![0, 1]);
    }

    #[tokio::test]
    async fn protocol_error_is_retried_until_the_budget_then_surfaced() {
        let (transport, log) = pool(vec![vec![Reply::Body("protocol")], vec![Reply::Body("protocol")]]);
        // Two nodes, both reject: cycle until MAX_PROTOCOL_ERRORS rejections.
        assert_eq!(post(&transport).await.unwrap(), b"protocol");
        assert_eq!(*log.lock().unwrap(), vec![0, 1, 0]);
    }

    #[tokio::test]
    async fn protocol_error_on_single_node_is_retried_in_place() {
        let (transport, log) = pool(vec![vec![Reply::Body("protocol")]]);
        assert_eq!(post(&transport).await.unwrap(), b"protocol");
        assert_eq!(*log.lock().unwrap(), vec![0, 0, 0]);
    }

    #[tokio::test]
    async fn protocol_error_does_not_exhaust_node_and_can_recover() {
        let (transport, log) = pool(vec![vec![Reply::Body("protocol")], vec![Reply::Body("ok")]]);
        assert_eq!(post(&transport).await.unwrap(), b"ok");
        assert_eq!(*log.lock().unwrap(), vec![0, 1]);
    }

    #[tokio::test]
    async fn transport_failures_do_not_consume_the_protocol_budget() {
        let (transport, log) = pool(vec![
            vec![Reply::TransportError],
            vec![Reply::Body("protocol")],
            vec![Reply::Body("protocol")],
            vec![Reply::Body("protocol")],
        ]);
        assert_eq!(post(&transport).await.unwrap(), b"protocol");
        // Node 0 exhausted by transport error; nodes 1-3 burn the 3 protocol retries.
        assert_eq!(*log.lock().unwrap(), vec![0, 1, 2, 3]);
    }

    #[tokio::test]
    async fn prefers_last_successful_node_on_subsequent_requests() {
        let (transport, log) = pool(vec![
            vec![Reply::TransportError],
            vec![Reply::Body("ok"), Reply::Body("ok")],
        ]);
        assert_eq!(post(&transport).await.unwrap(), b"ok");
        // Second request starts at node 1 directly, never re-probing dead node 0.
        assert_eq!(post(&transport).await.unwrap(), b"ok");
        assert_eq!(*log.lock().unwrap(), vec![0, 1, 1]);
    }

    #[tokio::test]
    async fn empty_pool_is_rejected() {
        assert!(FailoverTransport::<MockNode>::with_classifier(
            vec![],
            Arc::new(MarkerClassifier)
        )
        .is_none());
    }

    #[test]
    fn monero_classifier_recognises_response_shapes() {
        let classifier = MoneroStatusClassifier;
        let route = "send_raw_transaction";

        assert_eq!(
            classifier.classify(route, br#"{"status":"OK"}"#),
            ResponseClass::Success
        );
        assert_eq!(
            classifier.classify(route, br#"{"status":"Failed","reason":"tx invalid"}"#),
            ResponseClass::Protocol
        );
        assert_eq!(
            classifier.classify(route, br#"{"status":"BUSY"}"#),
            ResponseClass::Transport
        );
        assert_eq!(
            classifier.classify("json_rpc", br#"{"error":{"code":-1,"message":"oops"}}"#),
            ResponseClass::Transport
        );
        assert_eq!(
            classifier.classify("json_rpc", br#"{"result":{"status":"Failed"}}"#),
            ResponseClass::Protocol
        );
        assert_eq!(
            classifier.classify(route, b"not json"),
            ResponseClass::Transport
        );
        assert_eq!(
            classifier.classify("on_get_block_hash", br#""abcdef""#),
            ResponseClass::Success
        );
    }
}
