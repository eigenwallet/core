//! Retry utilities with exponential backoff.

use std::{fmt::Debug, time::Duration};

use backoff::backoff::Backoff as _;

pub struct Backoff(backoff::ExponentialBackoff);

impl Backoff {
    pub fn new() -> Self {
        let inner = backoff::ExponentialBackoff {
            initial_interval: Duration::from_secs(1),
            max_interval: Duration::from_secs(60),
            max_elapsed_time: None,
            ..Default::default()
        };
        Self(inner)
    }

    /// Reset the backoff to its initial state.
    ///
    /// Call this after a successful operation so that subsequent failures
    /// start from the initial interval rather than continuing from where
    /// a previous failure sequence left off.
    pub fn reset(&mut self) {
        self.0.reset();
    }

    pub async fn sleep_on_error(&mut self, err: &impl Debug, msg: &'static str) {
        let retry_after = self.0.next_backoff().expect("backoff never exhausts");

        tracing::warn!(
            error = ?err,
            retry_after_secs = retry_after.as_secs(),
            "{msg}"
        );

        tokio::time::sleep(retry_after).await;
    }
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new()
    }
}
