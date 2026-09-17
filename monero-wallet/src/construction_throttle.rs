use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::{Instant, sleep_until};

pub struct ConstructionThrottle {
    next_allowed: Mutex<Instant>,
    interval: Duration,
}

impl ConstructionThrottle {
    pub fn new(interval: Duration) -> Self {
        Self {
            next_allowed: Mutex::new(Instant::now()),
            interval,
        }
    }

    pub async fn wait_for_my_turn(&self) -> Instant {
        let mut next_allowed = self.next_allowed.lock().await;
        sleep_until(*next_allowed).await;
        *next_allowed = Instant::now() + self.interval;
        *next_allowed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use tokio::time::{advance, sleep, timeout};

    #[tokio::test(start_paused = true)]
    async fn admits_in_order_even_while_previous_construction_is_still_running() {
        let interval = Duration::from_secs(300);
        let throttle = ConstructionThrottle::new(interval);
        let started = Instant::now();
        let admissions = RefCell::new(Vec::new());

        tokio::join!(
            biased;
            async {
                throttle.wait_for_my_turn().await;
                admissions.borrow_mut().push((1, started.elapsed()));
                sleep(interval * 3).await;
            },
            async {
                throttle.wait_for_my_turn().await;
                admissions.borrow_mut().push((2, started.elapsed()));
            },
            async {
                throttle.wait_for_my_turn().await;
                admissions.borrow_mut().push((3, started.elapsed()));
            },
        );

        assert_eq!(
            *admissions.borrow(),
            [(1, Duration::ZERO), (2, interval), (3, interval * 2)]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn cancelled_waiters_leave_the_next_turn_available() {
        let interval = Duration::from_secs(300);
        let throttle = ConstructionThrottle::new(interval);
        throttle.wait_for_my_turn().await;
        let started = Instant::now();

        let (head, queued, ()) = tokio::join!(
            biased;
            timeout(Duration::from_secs(100), throttle.wait_for_my_turn()),
            timeout(Duration::from_secs(50), throttle.wait_for_my_turn()),
            async {
                throttle.wait_for_my_turn().await;
                assert_eq!(started.elapsed(), interval);
            },
        );

        assert!(head.is_err());
        assert!(queued.is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn idle_time_does_not_accumulate_turns() {
        let interval = Duration::from_secs(300);
        let throttle = ConstructionThrottle::new(interval);
        throttle.wait_for_my_turn().await;
        advance(interval * 3).await;
        let started = Instant::now();

        throttle.wait_for_my_turn().await;
        assert_eq!(started.elapsed(), Duration::ZERO);
        throttle.wait_for_my_turn().await;
        assert_eq!(started.elapsed(), interval);
    }

    #[tokio::test(start_paused = true)]
    async fn zero_disables_spacing() {
        let throttle = ConstructionThrottle::new(Duration::ZERO);
        let started = Instant::now();
        for _ in 0..3 {
            assert_eq!(throttle.wait_for_my_turn().await, started);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn an_expired_turn_requeues_behind_an_existing_waiter() {
        let interval = Duration::from_secs(300);
        let throttle = ConstructionThrottle::new(interval);
        let expires_at = throttle.wait_for_my_turn().await;
        advance(interval).await;

        let (other_turn, renewed_turn) = tokio::join!(
            biased;
            throttle.wait_for_my_turn(),
            throttle.wait_for_my_turn(),
        );

        assert_eq!(other_turn, expires_at + interval);
        assert_eq!(renewed_turn, expires_at + interval * 2);
        assert_eq!(Instant::now(), other_turn);
    }
}
