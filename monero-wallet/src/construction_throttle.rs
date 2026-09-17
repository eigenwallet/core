use anyhow::{Result, ensure};
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::{Instant, sleep_until};

pub struct ConstructionThrottle {
    next_allowed: Mutex<Instant>,
    interval: Duration,
}

impl ConstructionThrottle {
    pub fn new(interval: Duration) -> Result<Self> {
        ensure!(
            !interval.is_zero(),
            "Construction turn duration must be positive"
        );
        Ok(Self {
            next_allowed: Mutex::new(Instant::now()),
            interval,
        })
    }

    pub async fn wait_for_my_turn(&self) -> Duration {
        let mut next_allowed = self.next_allowed.lock().await;
        sleep_until(*next_allowed).await;
        *next_allowed = Instant::now() + self.interval;
        self.interval
    }
}
