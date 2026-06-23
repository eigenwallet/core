//! On-chain encrypted signature channel, an alternative to the p2p protocol:
//! Alice attaches a funding output for the Hermes wallet (spend key `s_b`,
//! view key `v`) to the Monero lock transaction. Bob spends it back to the
//! Hermes wallet with the encrypted signature embedded in tx_extra. Alice
//! scans the Hermes wallet and extracts the signature.

use anyhow::{Context, Result};
use monero_wallet_ng::hermes::HermesMessage;
use swap_core::bitcoin::EncryptedSignature;

pub fn encode_encrypted_signature(enc_sig: &EncryptedSignature) -> Result<HermesMessage> {
    let bytes =
        bincode::serialize(enc_sig).context("Failed to serialize the encrypted signature")?;

    HermesMessage::new(bytes)
        .context("Encrypted signature does not fit into a single Hermes message")
}

pub fn decode_encrypted_signature(message: &HermesMessage) -> Result<EncryptedSignature> {
    bincode::deserialize(message.as_bytes())
        .context("Failed to deserialize an encrypted signature from the Hermes message")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::bitcoin::hashes::Hash;
    use curve25519_dalek::scalar::Scalar as DalekScalar;
    use swap_core::bitcoin::SecretKey;
    use swap_core::compat::IntoMoneroOxide;
    use zeroize::Zeroizing;

    #[test]
    fn encrypted_signature_roundtrips_through_hermes() {
        let mut rng = rand::thread_rng();

        let b = SecretKey::new_random(&mut rng);
        let S_a = SecretKey::new_random(&mut rng).public();
        let digest = ::bitcoin::sighash::SegwitV0Sighash::from_byte_array([42u8; 32]);
        let enc_sig = b.encsign(S_a, digest);

        let message = encode_encrypted_signature(&enc_sig).unwrap();

        let view_key = Zeroizing::new(DalekScalar::from(7u64).into_monero_oxide());
        let parts = message.to_arbitrary_data(view_key.clone(), &mut rng);
        let received = HermesMessage::from_arbitrary_data(&parts, view_key).unwrap();

        let decoded = decode_encrypted_signature(&received).unwrap();
        assert_eq!(decoded, enc_sig);
    }
}
