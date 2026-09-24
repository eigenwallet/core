//! Lets the taker obtain a [`SwapAttestation`] from the maker: a signed statement
//! that both did a swap together which progressed at least to the Bitcoin being locked.

pub mod alice;
pub mod bob;

use serde::{Deserialize, Serialize};
use swap_machine::swap_attestation::SwapAttestation;
use uuid::Uuid;

const PROTOCOL: &str = "/comit/xmr/btc/swap_attestation/1.0.0";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub swap_id: Uuid,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Response {
    Attested(SwapAttestation),
    Rejected(SwapAttestationRejectReason),
}

#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SwapAttestationRejectReason {
    #[error("Alice does not have a record of the swap")]
    UnknownSwap,
    #[error("Alice did the swap with a different peer")]
    MaliciousRequest,
    #[error("Alice has not seen the Bitcoin being locked")]
    BtcNotLocked,
}

#[cfg(test)]
mod tests {
    use super::alice::{SwapAttestationSource, SwapRecord};
    use super::bob::{SwapAttestationStore, SwapAwaitingAttestation};
    use super::*;
    use crate::test::{SwarmExt, new_swarm};
    use ::bitcoin::hashes::Hash;
    use anyhow::{Result, anyhow};
    use async_trait::async_trait;
    use futures::StreamExt;
    use libp2p::PeerId;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use swap_machine::swap_attestation::SwapTerms;

    struct FakeSource<F> {
        lookup: F,
        lookups: AtomicUsize,
    }

    #[async_trait]
    impl<F> SwapAttestationSource for FakeSource<F>
    where
        F: Fn(usize) -> Result<Option<SwapRecord>> + Send + Sync,
    {
        async fn swap_attestation_record(&self, _: Uuid) -> Result<Option<SwapRecord>> {
            (self.lookup)(self.lookups.fetch_add(1, Ordering::SeqCst))
        }
    }

    #[derive(Default)]
    struct FakeStore {
        awaiting: Mutex<Vec<SwapAwaitingAttestation>>,
        stored: Mutex<Vec<SwapAttestation>>,
    }

    #[async_trait]
    impl SwapAttestationStore for FakeStore {
        async fn swaps_awaiting_attestation(&self) -> Result<Vec<SwapAwaitingAttestation>> {
            let stored = self.stored.lock().unwrap();
            Ok(self
                .awaiting
                .lock()
                .unwrap()
                .iter()
                .filter(|swap| !stored.iter().any(|a| a.swap.swap_id == swap.swap_id))
                .cloned()
                .collect())
        }

        async fn store_swap_attestation(&self, attestation: SwapAttestation) -> Result<()> {
            self.stored.lock().unwrap().push(attestation);
            Ok(())
        }
    }

    fn terms(btc_sat: u64) -> SwapTerms {
        SwapTerms {
            btc_amount: ::bitcoin::Amount::from_sat(btc_sat),
            xmr_amount: swap_core::monero::Amount::from_pico(1_000_000_000_000),
            btc_lock_txid: ::bitcoin::Txid::all_zeros(),
        }
    }

    fn fast_config() -> bob::Config {
        bob::Config {
            poll_interval: Duration::from_millis(200),
            retry_initial_interval: Duration::from_millis(50),
            retry_max_interval: Duration::from_millis(100),
        }
    }

    /// Runs Alice and Bob until Bob stored an attestation or the timeout elapsed.
    async fn run<F>(lookup: F, bob_terms: SwapTerms) -> (Option<SwapAttestation>, usize)
    where
        F: Fn(PeerId, usize) -> Result<Option<SwapRecord>> + Send + Sync + 'static,
    {
        let store = Arc::new(FakeStore::default());
        let mut bob = new_swarm(|identity| {
            bob::Behaviour::new(identity.public().to_peer_id(), store.clone(), fast_config())
        });
        let bob_peer_id = *bob.local_peer_id();

        let source = Arc::new(FakeSource {
            lookup: move |attempt| lookup(bob_peer_id, attempt),
            lookups: AtomicUsize::new(0),
        });
        let mut alice = new_swarm(|identity| alice::Behaviour::new(identity, source.clone(), None));
        let alice_peer_id = *alice.local_peer_id();
        let alice_addr = alice.listen_on_random_memory_address().await;
        bob.add_peer_address(alice_peer_id, alice_addr);

        store
            .awaiting
            .lock()
            .unwrap()
            .push(SwapAwaitingAttestation {
                swap_id: Uuid::new_v4(),
                maker: alice_peer_id,
                terms: bob_terms,
            });

        let alice_task = tokio::spawn(async move {
            loop {
                alice.select_next_some().await;
            }
        });
        let bob_task = tokio::spawn(async move {
            loop {
                bob.select_next_some().await;
            }
        });

        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while tokio::time::Instant::now() < deadline && store.stored.lock().unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        alice_task.abort();
        bob_task.abort();

        let stored = store.stored.lock().unwrap().first().cloned();
        (stored, source.lookups.load(Ordering::SeqCst))
    }

    #[tokio::test]
    async fn bob_stores_attestation_of_locked_swap() {
        let (stored, _) = run(
            |taker, _| {
                Ok(Some(SwapRecord {
                    taker,
                    btc_locked_terms: Some(terms(100_000)),
                }))
            },
            terms(100_000),
        )
        .await;

        let attestation = stored.expect("attestation to be stored");
        attestation.verify().unwrap();
        assert_eq!(attestation.swap.terms, terms(100_000));
    }

    #[tokio::test]
    async fn bob_requests_again_after_btc_not_locked() {
        let (stored, lookups) = run(
            |taker, attempt| {
                Ok(Some(SwapRecord {
                    taker,
                    btc_locked_terms: (attempt > 0).then(|| terms(100_000)),
                }))
            },
            terms(100_000),
        )
        .await;

        assert!(stored.is_some());
        assert_eq!(lookups, 2);
    }

    #[tokio::test]
    async fn bob_retries_after_network_failure() {
        let (stored, lookups) = run(
            |taker, attempt| {
                if attempt == 0 {
                    return Err(anyhow!("database unavailable"));
                }
                Ok(Some(SwapRecord {
                    taker,
                    btc_locked_terms: Some(terms(100_000)),
                }))
            },
            terms(100_000),
        )
        .await;

        assert!(stored.is_some());
        assert_eq!(lookups, 2);
    }

    #[tokio::test]
    async fn bob_does_not_store_attestation_with_unexpected_terms() {
        let (stored, lookups) = run(
            |taker, _| {
                Ok(Some(SwapRecord {
                    taker,
                    btc_locked_terms: Some(terms(1)),
                }))
            },
            terms(100_000),
        )
        .await;

        assert!(stored.is_none());
        assert!(lookups > 1, "Bob keeps requesting at every poll");
    }

    #[tokio::test]
    async fn alice_rejects_swap_done_with_another_peer() {
        let (stored, lookups) = run(
            |_, _| {
                Ok(Some(SwapRecord {
                    taker: PeerId::random(),
                    btc_locked_terms: Some(terms(100_000)),
                }))
            },
            terms(100_000),
        )
        .await;

        assert!(stored.is_none());
        assert!(lookups > 1, "Bob keeps requesting at every poll");
    }

    #[tokio::test]
    async fn alice_rejects_unknown_swap() {
        let (stored, lookups) = run(|_, _| Ok(None), terms(100_000)).await;

        assert!(stored.is_none());
        assert!(lookups > 1, "Bob keeps requesting at every poll");
    }
}
