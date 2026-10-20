pub mod alice;
pub mod bob;
pub mod common;

#[cfg(test)]
mod tests {
    use super::*;
    use ::bitcoin::hashes::Hash;
    use ::bitcoin::sighash::SegwitV0Sighash as Sighash;
    use bitcoin_wallet::*;
    use monero_oxide_ext::PrivateKey;
    use rand::rngs::OsRng;
    use swap_core::bitcoin::*;
    use swap_core::monero::{Scalar, TransferProof};
    use swap_env::env::{GetConfig, Regtest};
    use uuid::Uuid;

    #[test]
    fn spending_path_budget_is_one_third_with_an_absolute_ceiling() {
        for (amount, budget) in [
            (0, 0),
            (2, 0),
            (3, 1),
            (299_999, 99_999),
            (300_000, 100_000),
            (300_001, 100_000),
            (600_000, 100_000),
        ] {
            let limits =
                common::swap_fee_limits(SwapFeeRate(5000), Amount::from_sat(amount)).unwrap();
            assert_eq!(limits.max_path_fee, Amount::from_sat(budget));
        }
    }

    #[tokio::test]
    async fn calculate_transaction_weights() {
        for multiplier in [200, 230, 300, 1000] {
            let multiplier = RedeemFeeMultiplier::try_from(multiplier).unwrap();
            check_transaction_weights(Amount::ZERO, multiplier).await;
            check_transaction_weights(Amount::from_sat(100_000), multiplier).await;
        }
    }

    async fn check_transaction_weights(
        btc_amnesty_amount: Amount,
        multiplier: RedeemFeeMultiplier,
    ) {
        let alice_wallet = TestWalletBuilder::new(Amount::ONE_BTC.to_sat())
            .build()
            .await;
        let bob_wallet = TestWalletBuilder::new(Amount::ONE_BTC.to_sat())
            .build()
            .await;
        let spending_fee = Amount::from_sat(1_000);
        let btc_amount = Amount::from_sat(500_000);
        let should_publish_tx_refund = false;
        let xmr_amount = swap_core::monero::primitives::Amount::from_pico(10000);

        let tx_lock_fee = spending_fee;
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
            SwapFeeRate(5000),
            multiplier,
            common::swap_fee_limits(SwapFeeRate(5000), btc_amount).unwrap(),
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
            common::swap_fee_limits(SwapFeeRate(5000), btc_amount).unwrap(),
            tx_lock_fee,
        );

        let message0 = bob_state0.next_message().unwrap();

        let bob_key = message0.B;
        let refund_address = message0.refund_address.clone();
        let (_, alice_state1) = alice_state0.receive(message0).unwrap();
        let alice_message1 = alice_state1.next_message().unwrap();
        assert_eq!(alice_message1.fee_rate, SwapFeeRate(5000));
        let schedule = SwapFeeInputs {
            a: alice_message1.A,
            b: bob_key,
            redeem_address: &alice_message1.redeem_address,
            refund_address: &refund_address,
            punish_address: &alice_message1.punish_address,
            lock_amount: btc_amount,
            deposit: btc_amnesty_amount,
            redeem_multiplier: alice_message1.redeem_multiplier,
        }
        .calculate(
            alice_message1.fee_rate,
            common::swap_fee_limits(SwapFeeRate(5000), btc_amount).unwrap(),
        )
        .unwrap();
        let mut underpriced = alice_message1.clone();
        underpriced.fee_rate = SwapFeeRate(2499);
        assert!(
            bob_state0
                .clone()
                .receive(&bob_wallet, underpriced)
                .await
                .is_err()
        );

        for (rate, deposit) in [
            (15001, btc_amnesty_amount),
            (u64::MAX, btc_amnesty_amount),
            (5000, Amount::from_sat(1)),
            (5000, Amount::from_sat(u64::MAX)),
        ] {
            let mut invalid = alice_message1.clone();
            invalid.fee_rate = SwapFeeRate(rate);
            invalid.amnesty_amount = deposit;
            assert!(
                bob_state0
                    .clone()
                    .receive(&bob_wallet, invalid)
                    .await
                    .is_err()
            );
        }
        let message_json = serde_json::to_value(&alice_message1).unwrap();
        assert!(message_json.get("tx_redeem_fee").is_none());
        assert_eq!(message_json["fee_rate"], 5000);
        assert_eq!(message_json["redeem_multiplier"], u32::from(multiplier));
        assert!(message_json.get("fees").is_none());
        assert!(message_json.get("protocol_version").is_none());
        assert!(message_json.get("max_vsize").is_none());

        let bob_state1 = bob_state0
            .receive(&bob_wallet, alice_message1)
            .await
            .unwrap();
        let bob_message2 = bob_state1.next_message();

        let wrong_challenge = utxo_ownership_proof::challenge(b"another swap context");
        let wrong_proofs = bob_wallet
            .prove_utxo_ownership(&bob_message2.tx_lock_psbt, wrong_challenge)
            .unwrap();
        let wrong_verified = utxo_ownership_proof::VerifiedPsbt::verify(
            bob_message2.tx_lock_psbt.clone(),
            &wrong_proofs,
            wrong_challenge,
        )
        .unwrap();
        assert!(alice_state1.clone().receive(wrong_verified).is_err());
        let verified = utxo_ownership_proof::VerifiedPsbt::verify(
            bob_message2.tx_lock_psbt.clone(),
            &bob_message2.tx_lock_proofs,
            alice_state1.ownership_challenge(&bob_message2.tx_lock_psbt),
        )
        .unwrap();
        let mut different_message = message_json.clone();
        different_message["redeem_multiplier"] = serde_json::json!(199);
        assert!(serde_json::from_value::<common::Message1>(different_message).is_err());
        let alice_state2 = alice_state1.receive(verified).unwrap();
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
                PrivateKey::from_scalar(Scalar::ONE),
            )
            .into(),
            None,
        );
        let encrypted_signature = bob_state4.tx_redeem_encsig();
        let bob_state6 = bob_state4.cancel();

        let cancel_transaction = alice_state3.signed_cancel_transaction().unwrap();
        let punish_transaction = alice_state3.signed_punish_transaction().unwrap();
        let redeem_transaction = alice_state3
            .signed_redeem_transaction(encrypted_signature)
            .unwrap();
        let refund_transaction = bob_state6.construct_best_bitcoin_refund_tx().unwrap().0;

        assert_eq!(
            cancel_transaction.version,
            ::bitcoin::transaction::Version(3)
        );
        assert_eq!(
            refund_transaction.version,
            ::bitcoin::transaction::Version(3)
        );
        assert_eq!(
            refund_transaction.input[0].sequence,
            ::bitcoin::Sequence::from_height(1)
        );
        assert_eq!(
            btc_amount - redeem_transaction.output[0].value,
            multiplier
                .fee(btc_amount - cancel_transaction.output[0].value)
                .unwrap()
        );

        let base_fee = btc_amount - cancel_transaction.output[0].value;
        assert_eq!(
            cancel_transaction.output[0].value - punish_transaction.output[0].value,
            base_fee
        );
        assert_eq!(
            cancel_transaction.output[0].value
                - refund_transaction
                    .output
                    .iter()
                    .map(|o| o.value)
                    .sum::<Amount>(),
            base_fee
        );

        assert_eq!(base_fee, schedule.base);
        for tx in [
            &cancel_transaction,
            &redeem_transaction,
            &refund_transaction,
            &punish_transaction,
        ] {
            assert!(tx.vsize() as u64 <= schedule.max_vsize);
        }
        if btc_amnesty_amount != Amount::ZERO {
            let reclaim = bob_state6.signed_amnesty_transaction().unwrap();
            let withhold = alice_state3.signed_withhold_transaction().unwrap();
            let mercy = alice_state3.signed_mercy_transaction().unwrap();
            assert_eq!(btc_amnesty_amount - reclaim.output[0].value, base_fee);
            assert_eq!(btc_amnesty_amount - withhold.output[0].value, base_fee);
            assert_eq!(withhold.output[0].value - mercy.output[0].value, base_fee);
            for tx in [reclaim, withhold, mercy] {
                assert!(tx.vsize() as u64 <= schedule.max_vsize);
            }
        }

        // New stored swaps must rebuild the exact transactions and signatures.
        let saved = serde_json::to_value(&alice_state3).unwrap();
        assert!(saved.get("fee_rate").is_none());
        assert!(saved.get("redeem_multiplier").is_none());
        let restored: alice::State3 = serde_json::from_value(saved.clone()).unwrap();
        assert_eq!(restored.tx_redeem_fee, schedule.redeem);
        assert_eq!(
            restored.signed_cancel_transaction().unwrap(),
            cancel_transaction
        );
        let saved_bob = serde_json::to_value(&bob_state6).unwrap();
        assert!(saved_bob.get("fee_rate").is_none());
        assert!(saved_bob.get("redeem_multiplier").is_none());
        let restored_bob: bob::State6 = serde_json::from_value(saved_bob.clone()).unwrap();
        assert_eq!(
            restored_bob.construct_best_bitcoin_refund_tx().unwrap().0,
            refund_transaction
        );

        // Records written before this protocol had no format field.
        let mut legacy = saved;
        legacy
            .as_object_mut()
            .unwrap()
            .remove("cancel_refund_version");
        let legacy: alice::State3 = serde_json::from_value(legacy).unwrap();
        assert_ne!(legacy.tx_cancel().txid(), alice_state3.tx_cancel().txid());
        let mut legacy_bob = saved_bob;
        legacy_bob
            .as_object_mut()
            .unwrap()
            .remove("cancel_refund_version");
        let legacy_bob: bob::State6 = serde_json::from_value(legacy_bob).unwrap();
        assert_eq!(
            legacy_bob.cancel_refund_version(),
            CancelRefundVersion::Legacy
        );
        assert_eq!(
            legacy_bob.construct_tx_cancel().unwrap().txid(),
            legacy.tx_cancel().txid()
        );

        // A legacy record can have independent fees that violate the new ratio.
        // Its original cancel signature must still verify after recovery.
        let mut legacy_record = serde_json::to_value(&legacy).unwrap();
        legacy_record["tx_cancel_fee"] = serde_json::to_value(Amount::from_sat(1234)).unwrap();
        legacy_record["tx_redeem_fee"] = serde_json::to_value(Amount::from_sat(777)).unwrap();
        let legacy_unsigned: alice::State3 = serde_json::from_value(legacy_record.clone()).unwrap();
        let bob_secret: SecretKey =
            serde_json::from_value(serde_json::to_value(&bob_state6).unwrap()["b"].clone())
                .unwrap();
        legacy_record["tx_cancel_sig_bob"] =
            serde_json::to_value(bob_secret.sign(legacy_unsigned.tx_cancel().digest())).unwrap();
        let legacy_signed: alice::State3 = serde_json::from_value(legacy_record.clone()).unwrap();
        let original = legacy_signed.signed_cancel_transaction().unwrap();
        let recovered: alice::State3 =
            serde_json::from_str(&serde_json::to_string(&legacy_record).unwrap()).unwrap();
        assert_eq!(recovered.tx_cancel_fee, Amount::from_sat(1234));
        assert_eq!(recovered.tx_redeem_fee, Amount::from_sat(777));
        assert_eq!(recovered.signed_cancel_transaction().unwrap(), original);

        assert_weight(redeem_transaction, TxRedeem::weight().to_wu(), "TxRedeem");
        assert_weight(cancel_transaction, TxCancel::weight().to_wu(), "TxCancel");
        assert_weight(punish_transaction, TxPunish::weight().to_wu(), "TxPunish");
        assert_weight(
            refund_transaction,
            if btc_amnesty_amount == Amount::ZERO {
                TxFullRefund::weight().to_wu()
            } else {
                TxPartialRefund::weight().to_wu()
            },
            "TxRefund",
        );

        // Test TxEarlyRefund transaction
        let early_refund_transaction = alice_state3
            .signed_early_refund_transaction()
            .unwrap()
            .unwrap();
        assert_eq!(
            btc_amount - early_refund_transaction.output[0].value,
            base_fee
        );
        assert!(early_refund_transaction.vsize() as u64 <= schedule.max_vsize);
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
        let xmr_amount = swap_core::monero::primitives::Amount::from_pico(10000);

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
            SwapFeeRate(5000),
            RedeemFeeMultiplier::default(),
            common::swap_fee_limits(SwapFeeRate(5000), btc_amount).unwrap(),
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
            common::swap_fee_limits(SwapFeeRate(5000), btc_amount).unwrap(),
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

        let verified = utxo_ownership_proof::VerifiedPsbt::verify(
            bob_message2.tx_lock_psbt.clone(),
            &bob_message2.tx_lock_proofs,
            alice_state1.ownership_challenge(&bob_message2.tx_lock_psbt),
        )
        .unwrap();
        let alice_state2 = alice_state1.receive(verified).unwrap();
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
