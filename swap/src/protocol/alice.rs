//! Run an XMR/BTC swap in the role of Alice.
//! Alice holds XMR and wishes receive BTC.
use crate::protocol::Database;
pub use crate::protocol::alice::swap::*;
use crate::{asb, monero};
use bitcoin_wallet::BitcoinWallet;
use rust_decimal::Decimal;
use std::sync::Arc;
use swap_env::env::Config;
pub use swap_machine::alice::*;
use uuid::Uuid;

pub mod swap;

pub struct Swap {
    pub state: AliceState,
    pub event_loop_handle: asb::EventLoopHandle,
    pub bitcoin_wallet: Arc<dyn BitcoinWallet>,
    pub monero_wallet: Arc<monero::Wallets>,
    pub env_config: Config,
    pub developer_tip: TipConfig,
    pub hermes_funding_policy: HermesFundingPolicy,
    pub swap_id: Uuid,
    pub db: Arc<dyn Database + Send + Sync>,
    /// Reserves the TxLock inputs of a new swap until its first state is persisted
    pub lock_input_reservation: Option<crate::network::swap_setup::alice::LockInputReservation>,
}

/// How the maker funds Bob's on-chain Hermes encrypted-signature channel: the
/// extra Monero output attached to the lock transaction. Funding is skipped
/// when the channel is disabled or when the swap is below `min_swap_amount`.
#[derive(Clone, Copy, Debug)]
pub struct HermesFundingPolicy {
    pub enabled: bool,
    pub amount: monero::Amount,
    pub min_swap_amount: bitcoin::Amount,
}

impl HermesFundingPolicy {
    /// The Hermes funding amount for a swap of `swap_btc_amount`, or zero when
    /// the channel is disabled or the swap is below the minimum size.
    pub fn funding_amount(&self, swap_btc_amount: bitcoin::Amount) -> monero::Amount {
        if self.enabled && swap_btc_amount >= self.min_swap_amount {
            self.amount
        } else {
            monero::Amount::ZERO
        }
    }
}

/// Configures how much the and where the user wants to send tips to
///
/// The ratio is a number between 0 and 1
///
/// ratio = 0 means that no tip will be sent
/// ratio = 0.5 means that for a swap of 1 XMR, 0.5 XMR will be tipped
#[derive(Clone)]
pub struct TipConfig {
    pub ratio: Decimal,
    pub address: monero_address::MoneroAddress,
}

/// Alice's state after a complete swap setup with a Bob who funds the lock from `wallet`.
#[cfg(test)]
pub(crate) async fn test_state3(wallet: &dyn BitcoinWallet) -> anyhow::Result<State3> {
    use crate::protocol::bob;
    use swap_core::bitcoin::{
        Amount, CancelTimelock, PunishTimelock, RedeemFeeMultiplier, RemainingRefundTimelock,
        SwapFeeRate,
    };
    use swap_env::env::{GetConfig, Regtest};

    let mut rng = rand::rngs::OsRng;
    let env_config = Regtest::get_config();
    let btc = Amount::from_sat(500_000);
    let xmr = swap_core::monero::Amount::from_pico(10_000);
    let fee_rate = SwapFeeRate(5_000);
    let fee_limits = crate::protocol::swap_fee_limits(fee_rate, btc)?;

    let alice = State0::new(
        btc,
        xmr,
        Amount::ZERO,
        env_config,
        wallet.new_address().await?,
        wallet.new_address().await?,
        fee_rate,
        RedeemFeeMultiplier::default(),
        fee_limits,
        false,
        &mut rng,
    );
    let bob = bob::State0::new(
        Uuid::new_v4(),
        &mut rng,
        btc,
        xmr,
        CancelTimelock::new(env_config.bitcoin_cancel_timelock),
        PunishTimelock::new(env_config.bitcoin_punish_timelock),
        RemainingRefundTimelock::new(env_config.bitcoin_remaining_refund_timelock),
        wallet.new_address().await?,
        env_config.monero_finality_confirmations,
        fee_limits,
        Amount::from_sat(1_000),
    );

    let (_, alice) = alice.receive(bob.next_message()?)?;
    let bob = bob.receive(wallet, alice.next_message()?).await?;
    let message2 = bob.next_message();
    let challenge = alice.ownership_challenge(&message2.tx_lock_psbt);
    let verified = utxo_ownership_proof::VerifiedPsbt::verify(
        message2.tx_lock_psbt,
        &message2.tx_lock_proofs,
        challenge,
    )?;
    let alice = alice.receive(verified)?;
    let bob = bob.receive(alice.next_message()?)?;
    alice.receive(bob.next_message()?)
}
