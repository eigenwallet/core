use std::sync::Arc;

use anyhow::{Context, Result};
use uuid::Uuid;

use bitcoin_wallet;
use swap_core::monero::TxHash;
use swap_machine::bob::{State3, State4, State5};

use crate::cli::SwapEventLoopHandle;
use crate::common::retry;
use crate::monero;
use crate::monero::MoneroAddressPool;

pub(super) trait XmrRedeemable {
    async fn redeem_xmr(
        self,
        monero_wallet: &monero::Wallets,
        swap_id: Uuid,
        monero_receive_pool: MoneroAddressPool,
    ) -> Result<TxHash>;
}

impl XmrRedeemable for State5 {
    async fn redeem_xmr(
        self: State5,
        monero_wallet: &monero::Wallets,
        swap_id: Uuid,
        monero_receive_pool: MoneroAddressPool,
    ) -> Result<TxHash> {
        let (spend_key, view_key) = self.xmr_keys();

        tracing::info!(%swap_id, "Redeeming Monero");

        let wallet = monero_wallet
            .swap_wallet_spendable(
                swap_id,
                spend_key,
                view_key,
                self.lock_transfer_proof.tx_hash(),
            )
            .await
            .context("Failed to open Monero wallet")?;

        // Before we sweep, we ensure that the wallet is synchronized
        wallet.refresh_blocking().await?;

        tracing::debug!(%swap_id, receive_address=?monero_receive_pool, "Opened temporary Monero wallet, sweeping to receive address");

        let main_address = monero_wallet.main_wallet().await.main_address().await?;

        let tx_hash = wallet
            .sweep_multi_destination(
                &monero_receive_pool.fill_empty_addresses(main_address),
                &monero_receive_pool.percentages(),
            )
            .await
            .context("Failed to redeem Monero")?
            .txid;

        tracing::info!(%swap_id, %tx_hash, "Redeemed Monero");

        Ok(TxHash(tx_hash))
    }
}

pub(super) trait WaitForIncomingXmrLockTransaction {
    async fn wait_for_incoming_xmr_lock_transaction(
        &self,
        monero_wallet: &monero::Wallets,
        swap_id: Uuid,
        monero_wallet_restore_blockheight: monero::BlockHeight,
    ) -> monero::TxHash;
}

impl WaitForIncomingXmrLockTransaction for State3 {
    async fn wait_for_incoming_xmr_lock_transaction(
        &self,
        monero_wallet: &monero::Wallets,
        _swap_id: Uuid,
        monero_wallet_restore_blockheight: monero::BlockHeight,
    ) -> monero::TxHash {
        let (public_spend_key, private_view_key) = self.xmr_view_keys();

        retry(
            "Waiting for incoming XMR lock transaction",
            || async move {
                monero_wallet
                    .wait_for_incoming_transfer_ng(
                        public_spend_key,
                        private_view_key,
                        self.xmr_amount(),
                        monero_wallet_restore_blockheight,
                    )
                    .await
                    .map_err(backoff::Error::transient)
            },
            None,
            None,
        )
        .await
        .expect("we never stop retrying to wait for incoming XMR lock transaction")
    }
}

pub(super) trait VerifyXmrLockTransaction {
    async fn verify_xmr_lock_transaction(
        &self,
        monero_wallet: &monero::Wallets,
        tx_hash: monero::TxHash,
    ) -> Result<bool>;
}

impl VerifyXmrLockTransaction for State3 {
    async fn verify_xmr_lock_transaction(
        &self,
        monero_wallet: &monero::Wallets,
        tx_hash: monero::TxHash,
    ) -> Result<bool> {
        let (public_spend_key, private_view_key) = self.xmr_view_keys();
        let expected_amount = self.xmr_amount();

        monero_wallet
            .verify_transfer_ng(
                &tx_hash,
                public_spend_key,
                private_view_key,
                expected_amount,
            )
            .await
    }
}

pub(super) trait InfallibleVerifyXmrLockTransaction {
    async fn infallible_verify_xmr_lock_transaction(
        self,
        monero_wallet: Arc<monero::Wallets>,
        tx_hash: monero::TxHash,
    ) -> bool;
}

impl<T> InfallibleVerifyXmrLockTransaction for T
where
    T: VerifyXmrLockTransaction + Clone,
{
    async fn infallible_verify_xmr_lock_transaction(
        self,
        monero_wallet: Arc<monero::Wallets>,
        tx_hash: monero::TxHash,
    ) -> bool {
        let state_for_retry = self;

        retry(
            "Verifying Monero lock transaction",
            || {
                let state = state_for_retry.clone();
                let monero_wallet = monero_wallet.clone();
                let tx_hash = tx_hash.clone();

                async move {
                    state
                        .verify_xmr_lock_transaction(&*monero_wallet, tx_hash)
                        .await
                        .map_err(backoff::Error::transient)
                }
            },
            None,
            None,
        )
        .await
        .expect("we never stop retrying to verify Monero lock transaction")
    }
}

pub(super) trait WaitForXmrLockTransactionConfirmation {
    async fn infallible_wait_for_xmr_lock_confirmation(
        &self,
        monero_wallet: &monero::Wallets,
        tx_hash: monero::TxHash,
        confirmation_target: u64,
    ) -> Result<bool>;
}

impl WaitForXmrLockTransactionConfirmation for State3 {
    async fn infallible_wait_for_xmr_lock_confirmation(
        &self,
        monero_wallet: &monero::Wallets,
        tx_hash: monero::TxHash,
        confirmation_target: u64,
    ) -> Result<bool> {
        retry(
            "Waiting for XMR lock transaction confirmation",
            || {
                let tx_hash = tx_hash.clone();
                
                async move {
                    monero_wallet
                        .wait_until_confirmed_ng(&tx_hash, confirmation_target, None::<fn((u64, u64))>)
                        .await
                        .map(|_| true)
                        .map_err(backoff::Error::transient)
                }
            },
            None,
            None,
        )
        .await
    }
}

impl WaitForXmrLockTransactionConfirmation for State5 {
    async fn infallible_wait_for_xmr_lock_confirmation(
        &self,
        monero_wallet: &monero::Wallets,
        tx_hash: monero::TxHash,
        confirmation_target: u64,
    ) -> Result<bool> {
        retry(
            "Waiting for XMR lock transaction confirmation",
            || {
                let tx_hash = tx_hash.clone();
                async move {
                    monero_wallet
                        .wait_until_confirmed_ng(&tx_hash, confirmation_target, None::<fn((u64, u64))>)
                        .await
                        .map(|_| true)
                        .map_err(backoff::Error::transient)
                }
            },
            None,
            None,
        )
        .await
    }
}

pub(super) trait WaitForBtcRedeem {
    async fn infallible_wait_for_btc_redeem(
        &self,
        bitcoin_wallet: &dyn bitcoin_wallet::BitcoinWallet,
    ) -> State5;
}

impl WaitForBtcRedeem for State4 {
    async fn infallible_wait_for_btc_redeem(
        &self,
        bitcoin_wallet: &dyn bitcoin_wallet::BitcoinWallet,
    ) -> State5 {
        retry(
            "Waiting for Bitcoin redeem transaction",
            || {
                let state = self.clone();
                async move {
                    state
                        .watch_for_redeem_btc(bitcoin_wallet)
                        .await
                        .map_err(backoff::Error::transient)
                }
            },
            None,
            None,
        )
        .await
        .expect("we never stop retrying to wait for Bitcoin redeem transaction")
    }
}

pub(super) trait RecvTransferProof {
    async fn infallible_recv_transfer_proof(
        &self,
        event_loop_handle: &mut SwapEventLoopHandle,
    ) -> monero::TransferProof;
}

impl RecvTransferProof for State3 {
    async fn infallible_recv_transfer_proof(
        &self,
        event_loop_handle: &mut SwapEventLoopHandle,
    ) -> monero::TransferProof {
        // TODO: Use a cleaner retry mechanism here
        // We cannot use the retry function here because we need mut access to the handle
        // Maybe we can use some macro here?
        loop {
            match event_loop_handle.recv_transfer_proof().await {
                Ok(proof) => return proof,
                Err(e) => {
                    tracing::warn!("Failed to receive transfer proof: {:#}, retrying in 1s", e);
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
        }
    }
}
