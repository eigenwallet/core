//! A wrapper [`Transport`] that limits the amount of IO that flows through the
//! connections it produces.
//!
//! The limit is enforced with a shared [token bucket]: a single bucket is
//! created per direction (outgoing/incoming) when the transport is built, and
//! every connection draws from that same bucket. The limit is therefore
//! *global* across all connections of a node rather than per-connection, which
//! is what you usually want when capping bandwidth.
//!
//! The primary use case is limiting *outgoing* IO (uploads): the bytes we write
//! to our peers. Incoming IO can be limited too, but is left unlimited by
//! default.
//!
//! [token bucket]: https://en.wikipedia.org/wiki/Token_bucket

use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use futures::future::BoxFuture;
use futures::io::{AsyncRead, AsyncWrite};
use futures::{Future, FutureExt, TryFutureExt};
use futures_timer::Delay;
use libp2p::core::transport::{ListenerId, TransportError, TransportEvent};
use libp2p::{Multiaddr, Transport};

/// Configuration for the IO limits applied by [`IoLimitedTransport`].
///
/// All rates are expressed in bytes per second. `None` means "unlimited" for
/// that direction.
#[derive(Clone, Copy, Debug, Default)]
pub struct IoLimits {
    /// Maximum *outgoing* (write) throughput in bytes per second, shared across
    /// all connections. `None` disables the limit.
    pub outgoing_bytes_per_sec: Option<u64>,
    /// Maximum *incoming* (read) throughput in bytes per second, shared across
    /// all connections. `None` disables the limit.
    pub incoming_bytes_per_sec: Option<u64>,
    /// Burst capacity in bytes for both directions. This is the maximum number
    /// of bytes that can be sent/received in a single instant after a period of
    /// inactivity. Defaults to one second worth of the configured rate.
    pub burst_bytes: Option<u64>,
}

impl IoLimits {
    /// No limits in either direction. This is a zero-overhead pass-through.
    pub fn unlimited() -> Self {
        Self::default()
    }

    /// Limit only the outgoing (write) direction to `bytes_per_sec`.
    pub fn outgoing(bytes_per_sec: u64) -> Self {
        Self {
            outgoing_bytes_per_sec: Some(bytes_per_sec),
            ..Self::default()
        }
    }

    fn outgoing_limiter(&self) -> RateLimiter {
        RateLimiter::new(self.outgoing_bytes_per_sec, self.burst_bytes)
    }

    fn incoming_limiter(&self) -> RateLimiter {
        RateLimiter::new(self.incoming_bytes_per_sec, self.burst_bytes)
    }
}

/// A shared, cloneable token-bucket rate limiter.
///
/// Cloning yields a handle to the *same* bucket, so all clones share a single
/// global budget. An unlimited limiter holds no state and grants everything
/// immediately.
#[derive(Clone)]
struct RateLimiter {
    bucket: Option<Arc<Mutex<Bucket>>>,
}

struct Bucket {
    /// Maximum number of tokens (bytes) the bucket can hold.
    capacity: f64,
    /// Currently available tokens (bytes).
    tokens: f64,
    /// Tokens (bytes) added per second.
    refill_per_sec: f64,
    /// When the bucket was last refilled.
    last_refill: Instant,
}

/// The outcome of trying to acquire permission to transfer some bytes.
enum Acquire {
    /// `n` bytes may be transferred now (`0 <= n <= desired`).
    Ready(usize),
    /// No budget available; wait at least this long before trying again.
    Wait(Duration),
}

impl RateLimiter {
    /// Build a limiter for the given rate. A rate of `None` or `0` is unlimited.
    /// `burst` defaults to one second worth of the rate.
    fn new(bytes_per_sec: Option<u64>, burst: Option<u64>) -> Self {
        let Some(rate) = bytes_per_sec.filter(|rate| *rate > 0) else {
            return Self { bucket: None };
        };

        // The bucket must hold at least one token, otherwise it could never
        // grant a single byte.
        let capacity = burst.unwrap_or(rate).max(1) as f64;

        Self {
            bucket: Some(Arc::new(Mutex::new(Bucket {
                capacity,
                tokens: capacity,
                refill_per_sec: rate as f64,
                last_refill: Instant::now(),
            }))),
        }
    }

    /// Try to acquire budget for up to `desired` bytes.
    fn acquire(&self, desired: usize) -> Acquire {
        let Some(bucket) = &self.bucket else {
            return Acquire::Ready(desired);
        };
        if desired == 0 {
            return Acquire::Ready(0);
        }

        let mut bucket = bucket.lock().expect("rate limiter mutex not poisoned");

        // Refill the bucket based on the time that has passed.
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(bucket.last_refill).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * bucket.refill_per_sec).min(bucket.capacity);
        bucket.last_refill = now;

        if bucket.tokens >= 1.0 {
            let granted = (bucket.tokens.floor() as usize).min(desired);
            bucket.tokens -= granted as f64;
            Acquire::Ready(granted)
        } else {
            // Wait until at least one whole token has accrued.
            let needed = 1.0 - bucket.tokens;
            Acquire::Wait(Duration::from_secs_f64(needed / bucket.refill_per_sec))
        }
    }

    /// Return `n` previously-acquired tokens to the bucket. Used when fewer
    /// bytes were actually transferred than were acquired.
    fn refund(&self, n: usize) {
        if n == 0 {
            return;
        }
        let Some(bucket) = &self.bucket else {
            return;
        };
        let mut bucket = bucket.lock().expect("rate limiter mutex not poisoned");
        bucket.tokens = (bucket.tokens + n as f64).min(bucket.capacity);
    }
}

/// A [`Transport`] wrapper that rate-limits the IO of every connection it
/// produces. See the [module documentation](self) for details.
pub struct IoLimitedTransport<T> {
    inner: T,
    write_limiter: RateLimiter,
    read_limiter: RateLimiter,
}

impl<T> IoLimitedTransport<T> {
    /// Wrap `inner`, applying the given `limits` globally across all of its
    /// connections.
    pub fn new(inner: T, limits: &IoLimits) -> Self {
        Self {
            inner,
            write_limiter: limits.outgoing_limiter(),
            read_limiter: limits.incoming_limiter(),
        }
    }
}

impl<T> Transport for IoLimitedTransport<T>
where
    T: Transport + Unpin + 'static,
    T::Output: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    T::Error: 'static,
    T::Dial: Send + 'static,
    T::ListenerUpgrade: Send + 'static,
{
    type Output = LimitedStream<T::Output>;
    type Error = T::Error;
    type Dial = BoxFuture<'static, Result<Self::Output, Self::Error>>;
    type ListenerUpgrade = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn listen_on(
        &mut self,
        id: ListenerId,
        addr: Multiaddr,
    ) -> Result<(), TransportError<Self::Error>> {
        self.inner.listen_on(id, addr)
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        self.inner.remove_listener(id)
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let (write, read) = (self.write_limiter.clone(), self.read_limiter.clone());
        let dial = self.inner.dial(addr)?;
        Ok(dial
            .map_ok(move |stream| LimitedStream::new(stream, write, read))
            .boxed())
    }

    fn dial_as_listener(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        let (write, read) = (self.write_limiter.clone(), self.read_limiter.clone());
        let dial = self.inner.dial_as_listener(addr)?;
        Ok(dial
            .map_ok(move |stream| LimitedStream::new(stream, write, read))
            .boxed())
    }

    fn address_translation(&self, listen: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.inner.address_translation(listen, observed)
    }

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        let this = self.get_mut();
        let (write, read) = (this.write_limiter.clone(), this.read_limiter.clone());

        Pin::new(&mut this.inner).poll(cx).map(|event| {
            event.map_upgrade(move |upgrade| {
                upgrade
                    .map_ok(move |stream| LimitedStream::new(stream, write, read))
                    .boxed()
            })
        })
    }
}

/// A connection whose read and write throughput is bounded by shared
/// [`RateLimiter`]s.
pub struct LimitedStream<S> {
    inner: S,
    write_limiter: RateLimiter,
    read_limiter: RateLimiter,
    /// Timer used to wake the task when write budget should be available again.
    write_delay: Option<Delay>,
    /// Timer used to wake the task when read budget should be available again.
    read_delay: Option<Delay>,
}

impl<S> LimitedStream<S> {
    fn new(inner: S, write_limiter: RateLimiter, read_limiter: RateLimiter) -> Self {
        Self {
            inner,
            write_limiter,
            read_limiter,
            write_delay: None,
            read_delay: None,
        }
    }
}

/// Block on `limiter` until budget for at least one byte (capped at `desired`)
/// is available, arming `delay` to wake the task in the meantime.
///
/// Returns `Poll::Ready(n)` with the number of bytes that may be transferred,
/// or `Poll::Pending` after arming the timer.
fn poll_acquire(
    limiter: &RateLimiter,
    delay: &mut Option<Delay>,
    cx: &mut Context<'_>,
    desired: usize,
) -> Poll<usize> {
    loop {
        match limiter.acquire(desired) {
            Acquire::Ready(granted) => {
                *delay = None;
                return Poll::Ready(granted);
            }
            Acquire::Wait(duration) => {
                // Re-arm the timer to the freshly computed deadline. As time
                // passes the wait shrinks, so resetting is safe and keeps us
                // from over-sleeping.
                match delay {
                    Some(delay) => delay.reset(duration),
                    None => *delay = Some(Delay::new(duration)),
                }

                if Pin::new(delay.as_mut().expect("delay just set")).poll(cx).is_pending() {
                    return Poll::Pending;
                }
                // Timer fired: loop and try to acquire again.
            }
        }
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for LimitedStream<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();

        let granted =
            match poll_acquire(&this.write_limiter, &mut this.write_delay, cx, buf.len()) {
                Poll::Ready(granted) => granted,
                Poll::Pending => return Poll::Pending,
            };
        if granted == 0 {
            return Poll::Ready(Ok(0));
        }

        match Pin::new(&mut this.inner).poll_write(cx, &buf[..granted]) {
            Poll::Ready(Ok(written)) => {
                // Refund the budget we acquired but did not use.
                this.write_limiter.refund(granted - written);
                Poll::Ready(Ok(written))
            }
            other => {
                // Nothing was written; refund everything we acquired.
                this.write_limiter.refund(granted);
                other
            }
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_close(cx)
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for LimitedStream<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();

        let granted = match poll_acquire(&this.read_limiter, &mut this.read_delay, cx, buf.len()) {
            Poll::Ready(granted) => granted,
            Poll::Pending => return Poll::Pending,
        };
        if granted == 0 {
            return Poll::Ready(Ok(0));
        }

        match Pin::new(&mut this.inner).poll_read(cx, &mut buf[..granted]) {
            Poll::Ready(Ok(read)) => {
                // Refund the budget we acquired but did not use.
                this.read_limiter.refund(granted - read);
                Poll::Ready(Ok(read))
            }
            other => {
                this.read_limiter.refund(granted);
                other
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::io::Cursor;
    use futures::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn unlimited_grants_everything_immediately() {
        let limiter = RateLimiter::new(None, None);
        assert!(matches!(limiter.acquire(10_000), Acquire::Ready(10_000)));
    }

    #[test]
    fn bucket_grants_up_to_capacity_then_makes_you_wait() {
        // 1000 bytes/sec, burst of 100 bytes.
        let limiter = RateLimiter::new(Some(1000), Some(100));

        // The full burst is available immediately.
        assert!(matches!(limiter.acquire(1000), Acquire::Ready(100)));

        // Now the bucket is empty and we must wait.
        assert!(matches!(limiter.acquire(1000), Acquire::Wait(_)));
    }

    #[test]
    fn refund_returns_budget_to_the_bucket() {
        let limiter = RateLimiter::new(Some(1000), Some(100));

        let Acquire::Ready(granted) = limiter.acquire(100) else {
            panic!("expected budget to be available");
        };
        assert_eq!(granted, 100);

        // Without a refund the bucket is empty.
        assert!(matches!(limiter.acquire(50), Acquire::Wait(_)));

        // After refunding we can acquire again.
        limiter.refund(50);
        assert!(matches!(limiter.acquire(50), Acquire::Ready(50)));
    }

    #[tokio::test]
    async fn limited_write_is_throttled_to_the_configured_rate() {
        // 1000 bytes/sec, burst of 100 bytes. Writing 500 bytes must take at
        // least ~400ms: 100 bytes are free (burst), the remaining 400 are paced
        // at 1000 bytes/sec.
        let write_limiter = RateLimiter::new(Some(1000), Some(100));
        let read_limiter = RateLimiter::new(None, None);

        let mut stream = LimitedStream::new(Cursor::new(Vec::new()), write_limiter, read_limiter);

        let start = Instant::now();
        stream.write_all(&[0u8; 500]).await.unwrap();
        let elapsed = start.elapsed();

        assert!(
            elapsed >= Duration::from_millis(350),
            "writing should have been throttled, took {elapsed:?}"
        );
        assert_eq!(stream.inner.into_inner().len(), 500);
    }

    #[tokio::test]
    async fn limited_read_yields_all_bytes() {
        let write_limiter = RateLimiter::new(None, None);
        let read_limiter = RateLimiter::new(Some(1000), Some(100));

        let data = vec![7u8; 300];
        let mut stream =
            LimitedStream::new(Cursor::new(data.clone()), write_limiter, read_limiter);

        let mut out = Vec::new();
        stream.read_to_end(&mut out).await.unwrap();

        assert_eq!(out, data);
    }
}
