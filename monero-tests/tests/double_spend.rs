use monero_interface::{
    InterfaceError, ProvidesBlockchainMeta, ProvidesScannableBlocks, ScannableBlock,
};
use monero_oxide_wallet::{
    block::{Block, BlockHeader},
    ed25519::CompressedPoint,
    transaction::{Input, Pruned, Timelock, Transaction, TransactionPrefix},
};
use monero_wallet_ng::{HARDFORK_VERSION, double_spend::has_confirmed_conflict};
use std::ops::RangeInclusive;
use std::sync::atomic::{AtomicUsize, Ordering};

const CONFLICT: [u8; 32] = [1; 32];
const ORIGINAL: [u8; 32] = [2; 32];

fn block(height: usize, conflict_height: Option<usize>) -> ScannableBlock {
    let previous = if height == 0 {
        [0; 32]
    } else {
        block(height - 1, conflict_height).block.hash()
    };
    let conflict = conflict_height == Some(height);
    let prefix = TransactionPrefix {
        additional_timelock: Timelock::None,
        inputs: vec![Input::Gen(height)],
        outputs: vec![],
        extra: vec![],
    };
    let transaction = Transaction::<Pruned>::V1 {
        prefix: TransactionPrefix {
            inputs: vec![Input::ToKey {
                amount: None,
                key_offsets: vec![1],
                key_image: CompressedPoint::G,
            }],
            ..prefix.clone()
        },
        signatures: (),
    };
    ScannableBlock {
        block: Block::new(
            BlockHeader {
                hardfork_version: HARDFORK_VERSION,
                hardfork_signal: 0,
                timestamp: 0,
                previous,
                nonce: 0,
            },
            Transaction::V1 {
                prefix,
                signatures: vec![],
            },
            if conflict { vec![CONFLICT] } else { vec![] },
        )
        .unwrap(),
        transactions: if conflict { vec![transaction] } else { vec![] },
        output_index_for_first_ringct_output: Some(0),
    }
}

// Only the RPC methods used by the search are supplied; no wallet or blockchain process.
struct Blocks {
    tip: AtomicUsize,
    tip_after_fetch: Option<usize>,
    conflict_height: usize,
    replaced: bool,
    fail_fetch: bool,
    break_continuity_at: Option<usize>,
}

impl ProvidesBlockchainMeta for Blocks {
    async fn latest_block_number(&self) -> Result<usize, InterfaceError> {
        Ok(self.tip.load(Ordering::SeqCst))
    }
}
impl ProvidesScannableBlocks for Blocks {
    async fn contiguous_scannable_blocks(
        &self,
        range: RangeInclusive<usize>,
    ) -> Result<Vec<ScannableBlock>, InterfaceError> {
        if self.fail_fetch {
            return Err(InterfaceError::InvalidInterface("fetch failed".into()));
        }
        assert!(*range.end() <= self.tip.load(Ordering::SeqCst));
        assert!(range.clone().count() <= 10);
        if let Some(tip) = self.tip_after_fetch {
            self.tip.store(tip, Ordering::SeqCst);
        }
        Ok(range
            .map(|height| {
                let mut block = block(height, Some(self.conflict_height));
                if self.break_continuity_at == Some(height) {
                    block.block.header.previous = [9; 32];
                }
                block
            })
            .collect())
    }
    async fn scannable_block_by_number(
        &self,
        height: usize,
    ) -> Result<ScannableBlock, InterfaceError> {
        Ok(block(
            height,
            if self.replaced {
                None
            } else {
                Some(self.conflict_height)
            },
        ))
    }
    async fn scannable_block(&self, _: [u8; 32]) -> Result<ScannableBlock, InterfaceError> {
        panic!("not used")
    }
}

#[tokio::test]
async fn requires_exact_confirmation_depth() {
    for (tip, expected) in [(13, false), (23, false), (24, true), (25, true)] {
        let provider = Blocks {
            tip: tip.into(),
            tip_after_fetch: None,
            conflict_height: 10,
            replaced: false,
            fail_fetch: false,
            break_continuity_at: None,
        };
        assert_eq!(
            has_confirmed_conflict(&provider, ORIGINAL, &[CompressedPoint::G.to_bytes()], 0, 15)
                .await
                .unwrap(),
            expected
        );
    }
}

#[tokio::test]
async fn excludes_original_transaction_and_unrelated_inputs() {
    let provider = Blocks {
        tip: 19.into(),
        tip_after_fetch: None,
        conflict_height: 10,
        replaced: false,
        fail_fetch: false,
        break_continuity_at: None,
    };
    assert!(
        !has_confirmed_conflict(&provider, CONFLICT, &[CompressedPoint::G.to_bytes()], 0, 10)
            .await
            .unwrap()
    );
    assert!(
        !has_confirmed_conflict(&provider, ORIGINAL, &[[9; 32]], 0, 10)
            .await
            .unwrap()
    );
    assert!(
        has_confirmed_conflict(
            &provider,
            ORIGINAL,
            &[[9; 32], CompressedPoint::G.to_bytes()],
            0,
            10
        )
        .await
        .unwrap()
    );
}

#[tokio::test]
async fn rejects_reorganized_conflict_and_propagates_rpc_failure() {
    let mut provider = Blocks {
        tip: 19.into(),
        tip_after_fetch: None,
        conflict_height: 10,
        replaced: true,
        fail_fetch: false,
        break_continuity_at: None,
    };
    let error =
        has_confirmed_conflict(&provider, ORIGINAL, &[CompressedPoint::G.to_bytes()], 0, 10)
            .await
            .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Monero conflicting spend's block changed during verification"
    );
    provider.fail_fetch = true;
    assert!(
        has_confirmed_conflict(&provider, ORIGINAL, &[CompressedPoint::G.to_bytes()], 0, 10)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn respects_restore_height_and_rejects_zero_requirement() {
    let provider = Blocks {
        tip: 19.into(),
        tip_after_fetch: None,
        conflict_height: 10,
        replaced: false,
        fail_fetch: false,
        break_continuity_at: None,
    };
    assert!(
        !has_confirmed_conflict(
            &provider,
            ORIGINAL,
            &[CompressedPoint::G.to_bytes()],
            11,
            10
        )
        .await
        .unwrap()
    );
    assert!(
        has_confirmed_conflict(&provider, ORIGINAL, &[CompressedPoint::G.to_bytes()], 0, 0)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn rejects_a_restore_height_above_the_chain_tip() {
    for (tip, required_confirmations) in [(19, 10), (13, 15)] {
        let provider = Blocks {
            tip: tip.into(),
            tip_after_fetch: None,
            conflict_height: 10,
            replaced: false,
            fail_fetch: false,
            break_continuity_at: None,
        };
        let error = has_confirmed_conflict(
            &provider,
            ORIGINAL,
            &[CompressedPoint::G.to_bytes()],
            20,
            required_confirmations,
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("Conflict-search restore height 20 exceeds Monero chain tip {tip}")
        );
    }
}

#[tokio::test]
async fn errors_when_a_conflict_loses_confirmation_depth_during_verification() {
    for (new_tip, message) in [
        (
            18,
            "Monero conflicting spend lost the required confirmation depth during verification",
        ),
        (
            9,
            "Monero chain moved below the conflicting block during verification",
        ),
    ] {
        let provider = Blocks {
            tip: 19.into(),
            tip_after_fetch: Some(new_tip),
            conflict_height: 10,
            replaced: false,
            fail_fetch: false,
            break_continuity_at: None,
        };
        let error = has_confirmed_conflict(
            &provider,
            ORIGINAL,
            &[CompressedPoint::G.to_bytes()],
            10,
            10,
        )
        .await
        .unwrap_err();
        assert_eq!(error.to_string(), message);
    }
}

#[tokio::test]
async fn accepts_a_conflict_when_the_chain_advances_during_verification() {
    let provider = Blocks {
        tip: 19.into(),
        tip_after_fetch: Some(20),
        conflict_height: 10,
        replaced: false,
        fail_fetch: false,
        break_continuity_at: None,
    };
    assert!(
        has_confirmed_conflict(
            &provider,
            ORIGINAL,
            &[CompressedPoint::G.to_bytes()],
            10,
            10
        )
        .await
        .unwrap()
    );
}

#[tokio::test]
async fn errors_when_conflict_search_batches_do_not_connect() {
    let provider = Blocks {
        tip: 24.into(),
        tip_after_fetch: None,
        conflict_height: 20,
        replaced: false,
        fail_fetch: false,
        break_continuity_at: Some(10),
    };
    let error =
        has_confirmed_conflict(&provider, ORIGINAL, &[CompressedPoint::G.to_bytes()], 0, 15)
            .await
            .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Monero chain changed between conflict-search blocks"
    );
}

#[tokio::test]
async fn errors_when_a_negative_conflict_search_checkpoint_is_reorganized() {
    let provider = Blocks {
        tip: 19.into(),
        tip_after_fetch: None,
        conflict_height: 10,
        replaced: true,
        fail_fetch: false,
        break_continuity_at: None,
    };
    let error =
        has_confirmed_conflict(&provider, CONFLICT, &[CompressedPoint::G.to_bytes()], 0, 10)
            .await
            .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Monero conflict-search checkpoint changed during verification"
    );
}

#[tokio::test]
async fn accepts_a_negative_conflict_search_when_the_chain_advances() {
    let provider = Blocks {
        tip: 19.into(),
        tip_after_fetch: Some(20),
        conflict_height: 10,
        replaced: false,
        fail_fetch: false,
        break_continuity_at: None,
    };
    assert!(
        !has_confirmed_conflict(&provider, CONFLICT, &[CompressedPoint::G.to_bytes()], 0, 10)
            .await
            .unwrap()
    );
}
