use monero_wallet::construction_throttle::ConstructionThrottle;
use std::cell::RefCell;
use std::time::Duration;
use tokio::time::{Instant, advance, sleep, timeout};

#[tokio::test(start_paused = true)]
async fn admits_in_order_even_while_previous_construction_is_still_running() {
    let interval = Duration::from_secs(300);
    let throttle = ConstructionThrottle::new(interval).unwrap();
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
    let throttle = ConstructionThrottle::new(interval).unwrap();
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
    let throttle = ConstructionThrottle::new(interval).unwrap();
    throttle.wait_for_my_turn().await;
    advance(interval * 3).await;
    let started = Instant::now();

    throttle.wait_for_my_turn().await;
    assert_eq!(started.elapsed(), Duration::ZERO);
    throttle.wait_for_my_turn().await;
    assert_eq!(started.elapsed(), interval);
}

#[test]
fn rejects_zero_duration() {
    assert!(ConstructionThrottle::new(Duration::ZERO).is_err());
}

#[tokio::test(start_paused = true)]
async fn an_expired_turn_requeues_behind_an_existing_waiter() {
    let interval = Duration::from_secs(300);
    let throttle = ConstructionThrottle::new(interval).unwrap();
    let turn_duration = throttle.wait_for_my_turn().await;
    assert_eq!(turn_duration, interval);
    let started = Instant::now();
    advance(interval).await;

    let (other_turn, renewed_turn) = tokio::join!(
        biased;
        throttle.wait_for_my_turn(),
        throttle.wait_for_my_turn(),
    );

    assert_eq!(other_turn, interval);
    assert_eq!(renewed_turn, interval);
    assert_eq!(started.elapsed(), interval * 2);
}
