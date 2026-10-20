#[path = "harness/bitcoind.rs"]
mod bitcoind;

use anyhow::{Context, Result, ensure};
use bitcoin::consensus::encode::{deserialize_hex, serialize_hex};
use bitcoin::{Address, Amount, Network, Psbt, Transaction};
use serde_json::{Value, json};
use swap_core::bitcoin::{
    CancelRefundVersion, CancelTimelock, RedeemFeeMultiplier, SecretKey, TxCancel, TxFullRefund,
    TxLock, TxPartialRefund, TxRedeem, build_shared_output_descriptor,
};

struct Node {
    url: String,
    client: reqwest::Client,
}

impl Node {
    async fn rpc(&self, method: &str, params: Value) -> Result<Value> {
        let response: Value = self
            .client
            .post(&self.url)
            .basic_auth(bitcoind::RPC_USER, Some(bitcoind::RPC_PASSWORD))
            .json(&json!({"jsonrpc": "1.0", "id": 1, "method": method, "params": params}))
            .send()
            .await?
            .json()
            .await?;
        ensure!(
            response["error"].is_null(),
            "{method}: {}",
            response["error"]
        );
        Ok(response["result"].clone())
    }

    async fn address(&self) -> Result<Address> {
        let address = self.rpc("getnewaddress", json!([])).await?;
        Ok(address
            .as_str()
            .context("Missing address")?
            .parse::<Address<_>>()?
            .require_network(Network::Regtest)?)
    }
}

/// Exercise the signed swap transactions against standard Bitcoin Core policy.
/// A one-block sequence lock prevents refund fees from paying for cancel.
#[tokio::test]
async fn refund_requires_confirmed_cancel() -> Result<()> {
    let docker = testcontainers::clients::Cli::default();
    let container = docker.run(bitcoind::Bitcoind::default());
    let node = Node {
        url: format!(
            "http://127.0.0.1:{}",
            container.get_host_port_ipv4(bitcoind::RPC_PORT)
        ),
        client: reqwest::Client::new(),
    };
    node.rpc("createwallet", json!(["refund-delay"])).await?;
    let address = node.address().await?;
    node.rpc("generatetoaddress", json!([101, address])).await?;

    for (version, partial, redeem_multiplier) in [
        (CancelRefundVersion::Legacy, false, None),
        (CancelRefundVersion::Legacy, true, None),
        (CancelRefundVersion::Truc, false, None),
        (CancelRefundVersion::Truc, true, None),
        (CancelRefundVersion::Truc, false, Some(200)),
        (CancelRefundVersion::Truc, false, Some(230)),
        (CancelRefundVersion::Truc, false, Some(300)),
    ] {
        let a = SecretKey::new_random(&mut rand::thread_rng());
        let b = SecretKey::new_random(&mut rand::thread_rng());
        let descriptor = build_shared_output_descriptor(a.public().into(), b.public().into())?;
        let lock_address = descriptor.address(Network::Regtest)?;
        let lock_id = node
            .rpc("sendtoaddress", json!([lock_address, 0.01]))
            .await?;
        let lock_hex = node.rpc("getrawtransaction", json!([lock_id])).await?;
        let mut lock: Transaction =
            deserialize_hex(lock_hex.as_str().context("Missing lock hex")?)?;
        for input in &mut lock.input {
            input.script_sig = bitcoin::ScriptBuf::new();
            input.witness.clear();
        }
        let lock = TxLock::from_psbt(
            Psbt::from_unsigned_tx(lock)?,
            a.public(),
            b.public(),
            Amount::from_sat(1_000_000),
        )?;
        node.rpc("generatetoaddress", json!([2, address])).await?;

        let fee = Amount::from_sat(1000);
        let cancel = TxCancel::new(
            &lock,
            CancelTimelock::new(2),
            a.public(),
            b.public(),
            fee,
            version,
        )?;
        let refund = if partial {
            let refund = TxPartialRefund::new(
                &cancel,
                &address,
                a.public(),
                b.public(),
                Amount::from_sat(100_000),
                fee,
            )?;
            let digest = refund.digest();
            refund.add_signatures((a.public(), a.sign(digest)), (b.public(), b.sign(digest)))?
        } else {
            let refund = TxFullRefund::new(&cancel, &address, fee);
            let digest = refund.digest();
            refund.add_signatures((a.public(), a.sign(digest)), (b.public(), b.sign(digest)))?
        };
        let cancel_sig = b.sign(cancel.digest());
        let cancel = cancel.complete_as_alice(a.clone(), b.public(), cancel_sig)?;
        let expected_version = if version == CancelRefundVersion::Truc {
            3
        } else {
            2
        };
        assert_eq!(cancel.version.0, expected_version);
        assert_eq!(refund.version.0, expected_version);

        if let Some(hundredths) = redeem_multiplier {
            let secret = SecretKey::new_random(&mut rand::thread_rng());
            let redeem = TxRedeem::new(
                &lock,
                &address,
                RedeemFeeMultiplier::try_from(hundredths)?.fee(fee)?,
            );
            let encsig = b.encsign(secret.public(), redeem.digest());
            let redeem = redeem.complete(encsig, a, secret.into(), b.public())?;
            node.rpc("sendrawtransaction", json!([serialize_hex(&redeem)]))
                .await?;
            let result = node
                .rpc("testmempoolaccept", json!([[serialize_hex(&cancel)]]))
                .await?;
            assert_eq!(result[0]["allowed"], false, "{result}");
            node.rpc("generatetoaddress", json!([1, address])).await?;
            continue;
        }

        node.rpc("sendrawtransaction", json!([serialize_hex(&cancel)]))
            .await?;
        let before = node
            .rpc("testmempoolaccept", json!([[serialize_hex(&refund)]]))
            .await?;
        assert_eq!(
            before[0]["allowed"],
            version == CancelRefundVersion::Legacy,
            "{before}"
        );
        if version == CancelRefundVersion::Truc {
            assert_eq!(before[0]["reject-reason"], "non-BIP68-final");
            // Bob cannot remove the delay from Alice's signed refund.
            let mut no_delay = refund.clone();
            no_delay.input[0].sequence = bitcoin::Sequence::ZERO;
            let result = node
                .rpc("testmempoolaccept", json!([[serialize_hex(&no_delay)]]))
                .await?;
            assert_eq!(result[0]["allowed"], false, "{result}");
            assert!(
                result[0]["reject-reason"]
                    .as_str()
                    .context("Missing rejection reason")?
                    .contains("script"),
                "{result}"
            );
        }
        node.rpc("generatetoaddress", json!([1, address])).await?;
        let after = node
            .rpc("testmempoolaccept", json!([[serialize_hex(&refund)]]))
            .await?;
        assert_eq!(after[0]["allowed"], true, "{after}");
        node.rpc("sendrawtransaction", json!([serialize_hex(&refund)]))
            .await?;
        node.rpc("generatetoaddress", json!([1, address])).await?;
    }
    Ok(())
}
