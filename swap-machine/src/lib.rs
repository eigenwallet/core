pub mod alice;
pub mod bob;
pub mod common;

#[cfg(test)]
mod tests {
    use super::*;
    use ::bitcoin::hashes::Hash;
    use ::bitcoin::sighash::SegwitV0Sighash as Sighash;
    use bitcoin_wallet::*;
    use curve25519_dalek::scalar::Scalar;
    use monero::PrivateKey;
    use rand::rngs::OsRng;
    use swap_core::bitcoin::*;
    use swap_core::compat::IntoDalekNg;
    use swap_core::monero::TransferProof;
    use swap_env::env::{GetConfig, Regtest};
    use uuid::Uuid;

    #[tokio::test]
    async fn calculate_transaction_weights() {
        let alice_wallet = TestWalletBuilder::new(Amount::ONE_BTC.to_sat())
            .build()
            .await;
        let bob_wallet = TestWalletBuilder::new(Amount::ONE_BTC.to_sat())
            .build()
            .await;
        let spending_fee = Amount::from_sat(1_000);
        let btc_amount = Amount::from_sat(500_000);
        let btc_amnesty_amount = Amount::from_sat(100_000);
        let should_publish_tx_refund = false;
        let xmr_amount = swap_core::monero::primitives::Amount::from_piconero(10000);

        let tx_redeem_fee = alice_wallet
            .estimate_fee(TxRedeem::weight(), Some(btc_amount))
            .await
            .unwrap();
        let tx_punish_fee = alice_wallet
            .estimate_fee(TxPunish::weight(), Some(btc_amount))
            .await
            .unwrap();
        let tx_lock_fee = alice_wallet
            .estimate_fee(TxLock::weight(), Some(btc_amount))
            .await
            .unwrap();

        let redeem_address = alice_wallet.new_address().await.unwrap();
        let punish_address = alice_wallet.new_address().await.unwrap();

        let config = Regtest::get_config();
        let alice_state0 = alice::State0::new(
            btc_amount,
            xmr_amount,
            btc_amnesty_amount,
            config,
            redeem_address,
            punish_address,
            tx_redeem_fee,
            tx_punish_fee,
            spending_fee,
            should_publish_tx_refund,
            &mut OsRng,
        );

        let bob_state0 = bob::State0::new(
            Uuid::new_v4(),
            &mut OsRng,
            btc_amount,
            xmr_amount,
            CancelTimelock::new(config.bitcoin_cancel_timelock),
            PunishTimelock::new(config.bitcoin_punish_timelock),
            RemainingRefundTimelock::new(config.bitcoin_remaining_refund_timelock),
            bob_wallet.new_address().await.unwrap(),
            config.monero_finality_confirmations,
            spending_fee,
            spending_fee,
            spending_fee,
            spending_fee,
            spending_fee,
            tx_lock_fee,
        );

        let message0 = bob_state0.next_message().unwrap();

        let (_, alice_state1) = alice_state0.receive(message0).unwrap();
        let alice_message1 = alice_state1.next_message().unwrap();

        let bob_state1 = bob_state0
            .receive(&bob_wallet, alice_message1)
            .await
            .unwrap();
        let bob_message2 = bob_state1.next_message();

        let alice_state2 = alice_state1.receive(bob_message2).unwrap();
        let alice_message3 = alice_state2.next_message().unwrap();

        let bob_state2 = bob_state1.receive(alice_message3).unwrap();
        let bob_message4 = bob_state2.next_message().unwrap();

        let alice_state3 = alice_state2.receive(bob_message4).unwrap();

        let (bob_state3, _tx_lock) = bob_state2.lock_btc().await.unwrap();
        let bob_state4 = bob_state3.xmr_locked(
            swap_core::monero::BlockHeight { height: 0 },
            // We use bogus values here, because they're irrelevant to this test
            TransferProof::new(
                swap_core::monero::TxHash("foo".into()),
                PrivateKey::from_scalar(Scalar::random(&mut OsRng).into_dalek_ng()),
            )
            .into(),
        );
        let encrypted_signature = bob_state4.tx_redeem_encsig();
        let bob_state6 = bob_state4.cancel();

        let cancel_transaction = alice_state3.signed_cancel_transaction().unwrap();
        let punish_transaction = alice_state3.signed_punish_transaction().unwrap();
        let redeem_transaction = alice_state3
            .signed_redeem_transaction(encrypted_signature)
            .unwrap();
        let refund_transaction = bob_state6.signed_full_refund_transaction().unwrap();

        assert_weight(redeem_transaction, TxRedeem::weight().to_wu(), "TxRedeem");
        assert_weight(cancel_transaction, TxCancel::weight().to_wu(), "TxCancel");
        assert_weight(punish_transaction, TxPunish::weight().to_wu(), "TxPunish");
        assert_weight(
            refund_transaction,
            TxFullRefund::weight().to_wu(),
            "TxRefund",
        );

        // Test TxEarlyRefund transaction
        let early_refund_transaction = alice_state3
            .signed_early_refund_transaction()
            .unwrap()
            .unwrap();
        assert_weight(
            early_refund_transaction,
            TxEarlyRefund::weight() as u64,
            "TxEarlyRefund",
        );
    }

    #[tokio::test]
    async fn tx_early_refund_can_be_constructed_and_signed() {
        let alice_wallet = TestWalletBuilder::new(Amount::ONE_BTC.to_sat())
            .build()
            .await;
        let bob_wallet = TestWalletBuilder::new(Amount::ONE_BTC.to_sat())
            .build()
            .await;
        let spending_fee = Amount::from_sat(1_000);
        let btc_amount = Amount::from_sat(500_000);
        let btc_amnesty_amount = Amount::from_sat(100_000);
        let should_publish_tx_refund = false;
        let xmr_amount = swap_core::monero::primitives::Amount::from_piconero(10000);

        let tx_redeem_fee = alice_wallet
            .estimate_fee(TxRedeem::weight(), Some(btc_amount))
            .await
            .unwrap();
        let tx_punish_fee = alice_wallet
            .estimate_fee(TxPunish::weight(), Some(btc_amount))
            .await
            .unwrap();

        let refund_address = alice_wallet.new_address().await.unwrap();
        let punish_address = alice_wallet.new_address().await.unwrap();

        let config = Regtest::get_config();
        let alice_state0 = alice::State0::new(
            btc_amount,
            xmr_amount,
            btc_amnesty_amount,
            config,
            refund_address.clone(),
            punish_address,
            tx_redeem_fee,
            tx_punish_fee,
            spending_fee,
            should_publish_tx_refund,
            &mut OsRng,
        );

        let bob_state0 = bob::State0::new(
            Uuid::new_v4(),
            &mut OsRng,
            btc_amount,
            xmr_amount,
            CancelTimelock::new(config.bitcoin_cancel_timelock),
            PunishTimelock::new(config.bitcoin_punish_timelock),
            RemainingRefundTimelock::new(config.bitcoin_remaining_refund_timelock),
            bob_wallet.new_address().await.unwrap(),
            config.monero_finality_confirmations,
            spending_fee,
            spending_fee,
            spending_fee,
            spending_fee,
            spending_fee,
            spending_fee,
        );

        // Complete the state machine up to State3
        let message0 = bob_state0.next_message().unwrap();
        let (_, alice_state1) = alice_state0.receive(message0).unwrap();
        let alice_message1 = alice_state1.next_message().unwrap();

        let bob_state1 = bob_state0
            .receive(&bob_wallet, alice_message1)
            .await
            .unwrap();
        let bob_message2 = bob_state1.next_message();

        let alice_state2 = alice_state1.receive(bob_message2).unwrap();
        let alice_message3 = alice_state2.next_message().unwrap();

        let bob_state2 = bob_state1.receive(alice_message3).unwrap();
        let bob_message4 = bob_state2.next_message().unwrap();

        let alice_state3 = alice_state2.receive(bob_message4).unwrap();

        // Test TxEarlyRefund construction
        let tx_early_refund = alice_state3.tx_early_refund();

        // Verify basic properties
        assert_eq!(tx_early_refund.txid(), tx_early_refund.txid()); // Should be deterministic
        assert!(tx_early_refund.digest() != Sighash::all_zeros()); // Should have valid digest

        // Test that it can be signed and completed
        let early_refund_transaction = alice_state3
            .signed_early_refund_transaction()
            .unwrap()
            .unwrap();

        // Verify the transaction has expected structure
        assert_eq!(early_refund_transaction.input.len(), 1); // One input from lock tx
        assert_eq!(early_refund_transaction.output.len(), 1); // One output to refund address
        assert_eq!(
            early_refund_transaction.output[0].script_pubkey,
            refund_address.script_pubkey()
        );

        // Verify the input is spending the lock transaction
        assert_eq!(
            early_refund_transaction.input[0].previous_output,
            alice_state3.tx_lock.as_outpoint()
        );

        // Verify the amount is correct (lock amount minus fee)
        let expected_amount = alice_state3.tx_lock.lock_amount() - alice_state3.tx_refund_fee;
        assert_eq!(early_refund_transaction.output[0].value, expected_amount);
    }

    // Weights fluctuate because of the length of the signatures. Valid ecdsa
    // signatures can have 68, 69, 70, 71, or 72 bytes. Since most of our
    // transactions have 2 signatures the weight can be up to 8 bytes less than
    // the static weight (4 bytes per signature).
    fn assert_weight(transaction: Transaction, expected_weight: u64, tx_name: &str) {
        let is_weight = transaction.weight();

        assert!(
            expected_weight - is_weight.to_wu() <= 8,
            "{} to have weight {}, but was {}. Transaction: {:#?}",
            tx_name,
            expected_weight,
            is_weight,
            transaction
        )
    }
}
