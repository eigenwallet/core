use hkdf::Hkdf;
use sha2::Sha256;
use std::fmt;
use swap_core::{bitcoin, monero};
use uuid::Uuid;

/// Bob's recoverable secrets. A swap ID must never be reused for a different swap.
#[derive(Clone, PartialEq)]
pub struct SwapSecretKeys {
    pub(super) b: bitcoin::SecretKey,
    pub(super) s_b: monero::Scalar,
}

impl SwapSecretKeys {
    /// Derives independent keys from a secret seed and the public swap ID.
    ///
    /// HKDF-SHA256 gives the bytes of each key, and the bytes are reduced modulo the group order.
    /// This derivation is part of the backup format and must stay stable,
    /// so it does not use the key generators of our dependencies.
    /// Proofs and other protocol randomness must use a separate, fresh RNG.
    pub fn derive(seed: [u8; 32], swap_id: Uuid) -> Self {
        let hkdf = Hkdf::<Sha256>::new(Some(b"eigenwallet/bob-swap-keys/v1"), &seed);
        let mut bitcoin_bytes = [0; 32];
        let mut monero_bytes = [0; 64];
        hkdf.expand_multi_info(&[swap_id.as_bytes(), b"BITCOIN"], &mut bitcoin_bytes)
            .expect("32 bytes is a valid HKDF output length");
        hkdf.expand_multi_info(&[swap_id.as_bytes(), b"MONERO"], &mut monero_bytes)
            .expect("64 bytes is a valid HKDF output length");

        Self {
            b: bitcoin_key_from_bytes(bitcoin_bytes),
            s_b: monero_key_from_bytes(monero_bytes),
        }
    }
}

/// The secp256k1 group order is close to 2^256, so reducing 32 bytes gives no relevant bias.
fn bitcoin_key_from_bytes(bytes: [u8; 32]) -> bitcoin::SecretKey {
    let scalar: bitcoin::Scalar = bitcoin::Scalar::from_bytes_mod_order(bytes)
        .non_zero()
        .expect("a hash output does not reduce to zero");

    scalar.into()
}

/// The Ed25519 group order is close to 2^252, so the reduction takes 64 bytes to avoid a bias.
fn monero_key_from_bytes(bytes: [u8; 64]) -> monero::Scalar {
    let reduced = curve25519_dalek::Scalar::from_bytes_mod_order_wide(&bytes).to_bytes();

    monero::Scalar::read(&mut reduced.as_slice()).expect("a reduced scalar is canonical")
}

impl fmt::Debug for SwapSecretKeys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SwapSecretKeys(..)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derivation_matches_stable_backup_vector() {
        let keys = SwapSecretKeys::derive([7; 32], Uuid::nil());
        assert_eq!(
            keys.b.to_bytes(),
            [
                196, 25, 30, 171, 213, 225, 11, 152, 55, 216, 187, 108, 47, 65, 106, 55, 218, 212,
                35, 222, 73, 85, 7, 9, 89, 211, 53, 67, 5, 0, 218, 211,
            ]
        );
        let mut monero_bytes = Vec::new();
        keys.s_b.write(&mut monero_bytes).unwrap();
        assert_eq!(
            monero_bytes,
            [
                24, 108, 164, 63, 169, 126, 43, 205, 122, 185, 189, 197, 27, 214, 163, 123, 205,
                131, 143, 194, 238, 40, 105, 207, 191, 73, 227, 125, 227, 120, 128, 9,
            ]
        );
    }

    #[test]
    fn changing_seed_or_swap_id_changes_both_keys() {
        let keys = SwapSecretKeys::derive([7; 32], Uuid::nil());
        for other in [
            SwapSecretKeys::derive([8; 32], Uuid::nil()),
            SwapSecretKeys::derive([7; 32], Uuid::from_u128(1)),
        ] {
            assert_ne!(keys.b, other.b);
            assert_ne!(keys.s_b, other.s_b);
        }
        assert_eq!(format!("{keys:?}"), "SwapSecretKeys(..)");
    }
}
