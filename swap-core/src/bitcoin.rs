mod cancel;
mod early_refund;
mod lock;
mod punish;
mod redeem;
mod refund;
mod timelocks;

pub use crate::bitcoin::cancel::TxCancel;
pub use crate::bitcoin::early_refund::TxEarlyRefund;
pub use crate::bitcoin::lock::TxLock;
pub use crate::bitcoin::punish::TxPunish;
pub use crate::bitcoin::redeem::TxRedeem;
pub use crate::bitcoin::refund::TxRefund;
pub use crate::bitcoin::timelocks::{BlockHeight, ExpiredTimelocks};
pub use crate::bitcoin::timelocks::{CancelTimelock, PunishTimelock};
pub use ::bitcoin::amount::Amount;
pub use ::bitcoin::psbt::Psbt as PartiallySignedTransaction;
pub use ::bitcoin::{Address, AddressType, Network, Transaction, Txid};
pub use ecdsa_fun::adaptor::EncryptedSignature;
pub use ecdsa_fun::fun::Scalar;
pub use ecdsa_fun::Signature;

use ::bitcoin::hashes::Hash;
use ::bitcoin::secp256k1::ecdsa;
use ::bitcoin::sighash::SegwitV0Sighash as Sighash;
use anyhow::{bail, Context, Result};
use bdk_wallet::miniscript::descriptor::Wsh;
use bdk_wallet::miniscript::{Descriptor, Segwitv0};
use bitcoin_wallet::primitives::ScriptStatus;
use ecdsa_fun::adaptor::{Adaptor, HashTranscript};
use ecdsa_fun::fun::Point;
use ecdsa_fun::nonce::Deterministic;
use ecdsa_fun::ECDSA;
use rand::{CryptoRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::str::FromStr;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct SecretKey {
    inner: Scalar,
    public: Point,
}

impl SecretKey {
    pub fn new_random<R: RngCore + CryptoRng>(rng: &mut R) -> Self {
        let scalar = Scalar::random(rng);

        let ecdsa = ECDSA::<()>::default();
        let public = ecdsa.verification_key_for(&scalar);

        Self {
            inner: scalar,
            public,
        }
    }

    pub fn public(&self) -> PublicKey {
        PublicKey(self.public)
    }

    pub fn to_bytes(&self) -> [u8; 32] {
        self.inner.to_bytes()
    }

    pub fn sign(&self, digest: Sighash) -> Signature {
        let ecdsa = ECDSA::<Deterministic<Sha256>>::default();

        ecdsa.sign(&self.inner, &digest.to_byte_array())
    }

    // TxRefund encsigning explanation:
    //
    // A and B, are the Bitcoin Public Keys which go on the joint output for
    // TxLock_Bitcoin. S_a and S_b, are the Monero Public Keys which go on the
    // joint output for TxLock_Monero

    // tx_refund: multisig(A, B), published by bob
    // bob can produce sig on B using b
    // alice sends over an encrypted signature on A encrypted with S_b
    // s_b is leaked to alice when bob publishes signed tx_refund allowing her to
    // recover s_b: recover(encsig, S_b, sig_tx_refund) = s_b
    // alice now has s_a and s_b and can refund monero

    // self = a, Y = S_b, digest = tx_refund
    pub fn encsign(&self, Y: PublicKey, digest: Sighash) -> EncryptedSignature {
        let adaptor = Adaptor::<
            HashTranscript<Sha256, rand_chacha::ChaCha20Rng>,
            Deterministic<Sha256>,
        >::default();

        adaptor.encrypted_sign(&self.inner, &Y.0, &digest.to_byte_array())
    }
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PublicKey(Point);

impl PublicKey {
    #[cfg(test)]
    pub fn random() -> Self {
        Self(Point::random(&mut rand::thread_rng()))
    }
}

impl From<PublicKey> for Point {
    fn from(from: PublicKey) -> Self {
        from.0
    }
}

impl TryFrom<PublicKey> for bitcoin::PublicKey {
    type Error = bitcoin::key::FromSliceError;

    fn try_from(pubkey: PublicKey) -> Result<Self, Self::Error> {
        let bytes = pubkey.0.to_bytes();
        bitcoin::PublicKey::from_slice(&bytes)
    }
}

impl TryFrom<bitcoin::PublicKey> for PublicKey {
    type Error = anyhow::Error;

    fn try_from(pubkey: bitcoin::PublicKey) -> Result<Self, Self::Error> {
        let bytes = pubkey.to_bytes();
        let bytes_array: [u8; 33] = bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid public key length"))?;
        let point = Point::from_bytes(bytes_array)
            .ok_or_else(|| anyhow::anyhow!("Invalid public key bytes"))?;
        Ok(PublicKey(point))
    }
}

impl From<Point> for PublicKey {
    fn from(p: Point) -> Self {
        Self(p)
    }
}

impl From<Scalar> for SecretKey {
    fn from(scalar: Scalar) -> Self {
        let ecdsa = ECDSA::<()>::default();
        let public = ecdsa.verification_key_for(&scalar);

        Self {
            inner: scalar,
            public,
        }
    }
}

impl From<SecretKey> for Scalar {
    fn from(sk: SecretKey) -> Self {
        sk.inner
    }
}

impl From<Scalar> for PublicKey {
    fn from(scalar: Scalar) -> Self {
        let ecdsa = ECDSA::<()>::default();
        PublicKey(ecdsa.verification_key_for(&scalar))
    }
}

pub fn verify_sig(
    verification_key: &PublicKey,
    transaction_sighash: &Sighash,
    sig: &Signature,
) -> Result<()> {
    let ecdsa = ECDSA::verify_only();

    if ecdsa.verify(
        &verification_key.0,
        &transaction_sighash.to_byte_array(),
        sig,
    ) {
        Ok(())
    } else {
        bail!(InvalidSignature)
    }
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("signature is invalid")]
pub struct InvalidSignature;

pub fn verify_encsig(
    verification_key: PublicKey,
    encryption_key: PublicKey,
    digest: &Sighash,
    encsig: &EncryptedSignature,
) -> Result<()> {
    let adaptor = Adaptor::<HashTranscript<Sha256>, Deterministic<Sha256>>::default();

    if adaptor.verify_encrypted_signature(
        &verification_key.0,
        &encryption_key.0,
        &digest.to_byte_array(),
        encsig,
    ) {
        Ok(())
    } else {
        bail!(InvalidEncryptedSignature)
    }
}

#[derive(Clone, Copy, Debug, thiserror::Error)]
#[error("encrypted signature is invalid")]
pub struct InvalidEncryptedSignature;

pub fn build_shared_output_descriptor(
    A: Point,
    B: Point,
) -> Result<Descriptor<bitcoin::PublicKey>> {
    const MINISCRIPT_TEMPLATE: &str = "c:and_v(v:pk(A),pk_k(B))";

    let miniscript = MINISCRIPT_TEMPLATE
        .replace('A', &A.to_string())
        .replace('B', &B.to_string());

    let miniscript =
        bdk_wallet::miniscript::Miniscript::<bitcoin::PublicKey, Segwitv0>::from_str(&miniscript)
            .expect("a valid miniscript");

    Ok(Descriptor::Wsh(Wsh::new(miniscript)?))
}

pub fn recover(S: PublicKey, sig: Signature, encsig: EncryptedSignature) -> Result<SecretKey> {
    let adaptor = Adaptor::<HashTranscript<Sha256>, Deterministic<Sha256>>::default();

    let s = adaptor
        .recover_decryption_key(&S.0, &sig, &encsig)
        .map(SecretKey::from)
        .context("Failed to recover secret from adaptor signature")?;

    Ok(s)
}

pub fn current_epoch(
    cancel_timelock: CancelTimelock,
    punish_timelock: PunishTimelock,
    tx_lock_status: ScriptStatus,
    tx_cancel_status: ScriptStatus,
) -> ExpiredTimelocks {
    if tx_cancel_status.is_confirmed_with(punish_timelock) {
        return ExpiredTimelocks::Punish;
    }

    if tx_lock_status.is_confirmed_with(cancel_timelock) {
        return ExpiredTimelocks::Cancel {
            blocks_left: tx_cancel_status.blocks_left_until(punish_timelock),
        };
    }

    ExpiredTimelocks::None {
        blocks_left: tx_lock_status.blocks_left_until(cancel_timelock),
    }
}

pub mod bitcoin_address {
    use anyhow::{Context, Result};
    use bitcoin::{
        address::{NetworkChecked, NetworkUnchecked},
        Address,
    };
    use serde::Serialize;
    use std::str::FromStr;

    #[derive(thiserror::Error, Debug, Clone, Copy, PartialEq, Serialize)]
    #[error(
        "Invalid Bitcoin address provided, expected address on network {expected:?}  but address provided is on {actual:?}"
    )]
    pub struct BitcoinAddressNetworkMismatch {
        #[serde(with = "swap_serde::bitcoin::network")]
        expected: bitcoin::Network,
        #[serde(with = "swap_serde::bitcoin::network")]
        actual: bitcoin::Network,
    }

    pub fn parse(addr_str: &str) -> Result<bitcoin::Address<NetworkUnchecked>> {
        let address = bitcoin::Address::from_str(addr_str)?;

        if address.assume_checked_ref().address_type() != Some(bitcoin::AddressType::P2wpkh) {
            anyhow::bail!("Invalid Bitcoin address provided, only bech32 format is supported!")
        }

        Ok(address)
    }

    /// Parse the address and validate the network.
    pub fn parse_and_validate_network(
        address: &str,
        expected_network: bitcoin::Network,
    ) -> Result<bitcoin::Address> {
        let addres = bitcoin::Address::from_str(address)?;
        let addres = addres.require_network(expected_network).with_context(|| {
            format!("Bitcoin address network mismatch, expected `{expected_network:?}`")
        })?;
        Ok(addres)
    }

    /// Parse the address and validate the network.
    pub fn parse_and_validate(address: &str, is_testnet: bool) -> Result<bitcoin::Address> {
        let expected_network = if is_testnet {
            bitcoin::Network::Testnet
        } else {
            bitcoin::Network::Bitcoin
        };
        parse_and_validate_network(address, expected_network)
    }

    /// Validate the address network.
    pub fn validate(
        address: Address<NetworkUnchecked>,
        is_testnet: bool,
    ) -> Result<Address<NetworkChecked>> {
        let expected_network = if is_testnet {
            bitcoin::Network::Testnet
        } else {
            bitcoin::Network::Bitcoin
        };
        validate_network(address, expected_network)
    }

    /// Validate the address network.
    pub fn validate_network(
        address: Address<NetworkUnchecked>,
        expected_network: bitcoin::Network,
    ) -> Result<Address<NetworkChecked>> {
        address
            .require_network(expected_network)
            .context("Bitcoin address network mismatch")
    }

    /// Validate the address network even though the address is already checked.
    pub fn revalidate_network(
        address: Address,
        expected_network: bitcoin::Network,
    ) -> Result<Address> {
        address
            .as_unchecked()
            .clone()
            .require_network(expected_network)
            .context("bitcoin address network mismatch")
    }

    /// Validate the address network even though the address is already checked.
    pub fn revalidate(address: Address, is_testnet: bool) -> Result<Address> {
        revalidate_network(
            address,
            if is_testnet {
                bitcoin::Network::Testnet
            } else {
                bitcoin::Network::Bitcoin
            },
        )
    }
}

// Transform the ecdsa der signature bytes into a secp256kfun ecdsa signature type.
pub fn extract_ecdsa_sig(sig: &[u8]) -> Result<Signature> {
    let data = &sig[..sig.len() - 1];
    let sig = ecdsa::Signature::from_der(data)?.serialize_compact();
    Signature::from_bytes(sig).ok_or(anyhow::anyhow!("invalid signature"))
}

/// Bitcoin error codes: https://github.com/bitcoin/bitcoin/blob/97d3500601c1d28642347d014a6de1e38f53ae4e/src/rpc/protocol.h#L23
pub enum RpcErrorCode {
    /// Transaction or block was rejected by network rules. Error code -26.
    RpcVerifyRejected,
    /// Transaction or block was rejected by network rules. Error code -27.
    RpcVerifyAlreadyInChain,
    /// General error during transaction or block submission
    RpcVerifyError,
    /// Invalid address or key. Error code -5. Is throwns when a transaction is not found.
    /// See:
    /// - https://github.com/bitcoin/bitcoin/blob/ae024137bda9fe189f4e7ccf26dbaffd44cbbeb6/src/rpc/mempool.cpp#L470-L472
    /// - https://github.com/bitcoin/bitcoin/blob/ae024137bda9fe189f4e7ccf26dbaffd44cbbeb6/src/rpc/rawtransaction.cpp#L352-L368
    RpcInvalidAddressOrKey,
}

impl From<RpcErrorCode> for i64 {
    fn from(code: RpcErrorCode) -> Self {
        match code {
            RpcErrorCode::RpcVerifyError => -25,
            RpcErrorCode::RpcVerifyRejected => -26,
            RpcErrorCode::RpcVerifyAlreadyInChain => -27,
            RpcErrorCode::RpcInvalidAddressOrKey => -5,
        }
    }
}

pub fn parse_rpc_error_code(error: &anyhow::Error) -> anyhow::Result<i64> {
    // First try to extract an Electrum error from a MultiError if present
    if let Some(multi_error) = error.downcast_ref::<electrum_pool::MultiError>() {
        // Try to find the first Electrum error in the MultiError
        for single_error in multi_error.iter() {
            if let bdk_electrum::electrum_client::Error::Protocol(serde_json::Value::String(
                string,
            )) = single_error
            {
                let json = serde_json::from_str(
                    &string
                        .replace("sendrawtransaction RPC error:", "")
                        .replace("daemon error:", ""),
                )?;

                let json_map = match json {
                    serde_json::Value::Object(map) => map,
                    _ => continue, // Try next error if this one isn't a JSON object
                };

                let error_code_value = match json_map.get("code") {
                    Some(val) => val,
                    None => continue, // Try next error if no error code field
                };

                let error_code_number = match error_code_value {
                    serde_json::Value::Number(num) => num,
                    _ => continue, // Try next error if error code isn't a number
                };

                if let Some(int) = error_code_number.as_i64() {
                    return Ok(int);
                }
            }
        }
        // If we couldn't extract an RPC error code from any error in the MultiError
        bail!(
            "Error is of incorrect variant. We expected an Electrum error, but got: {}",
            error
        );
    }

    // Original logic for direct Electrum errors
    let string = match error.downcast_ref::<bdk_electrum::electrum_client::Error>() {
        Some(bdk_electrum::electrum_client::Error::Protocol(serde_json::Value::String(string))) => {
            string
        }
        _ => bail!(
            "Error is of incorrect variant. We expected an Electrum error, but got: {}",
            error
        ),
    };

    let json = serde_json::from_str(
        &string
            .replace("sendrawtransaction RPC error:", "")
            .replace("daemon error:", ""),
    )?;

    let json_map = match json {
        serde_json::Value::Object(map) => map,
        _ => bail!("Json error is not json object "),
    };

    let error_code_value = match json_map.get("code") {
        Some(val) => val,
        None => bail!("No error code field"),
    };

    let error_code_number = match error_code_value {
        serde_json::Value::Number(num) => num,
        _ => bail!("Error code is not a number"),
    };

    if let Some(int) = error_code_number.as_i64() {
        Ok(int)
    } else {
        bail!("Error code is not an unsigned integer")
    }
}

#[derive(Clone, Copy, thiserror::Error, Debug)]
#[error("transaction does not spend anything")]
pub struct NoInputs;

#[derive(Clone, Copy, thiserror::Error, Debug)]
#[error("transaction has {0} inputs, expected 1")]
pub struct TooManyInputs(usize);

#[derive(Clone, Copy, thiserror::Error, Debug)]
#[error("empty witness stack")]
pub struct EmptyWitnessStack;

#[derive(Clone, Copy, thiserror::Error, Debug)]
#[error("input has {0} witnesses, expected 3")]
pub struct NotThreeWitnesses(usize);
