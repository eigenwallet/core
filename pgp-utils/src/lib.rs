use std::io::{BufReader, Read};

use anyhow::{Context, Result};
use chrono::Utc;
use pgp::{
    composed::{
        ArmorOptions, KeyType, Message, SecretKeyParamsBuilder, SignedPublicKey, SignedSecretKey,
        SubkeyParamsBuilder,
    },
    types::{KeyDetails, Password},
};
use rand_chacha::rand_core::SeedableRng;

/// Our wrapper around a valid pgp key.
pub struct PgpKey {
    inner: SignedSecretKey,
}

impl PgpKey {
    /// Try to derive a pgp key from a specified seed.
    pub async fn try_from_seed(seed_bytes: [u8; 32]) -> Result<PgpKey> {
        // Seed the randomness generator with our private key
        let mut seeded_rng = rand_chacha::ChaCha20Rng::from_seed(seed_bytes);

        // Set up builders for subkeys
        let mut signkey = SubkeyParamsBuilder::default();
        signkey
            .key_type(KeyType::Ed25519Legacy)
            .can_sign(true)
            .can_encrypt(false)
            .can_authenticate(false);

        let mut encryptkey = SubkeyParamsBuilder::default();
        encryptkey
            .key_type(KeyType::ECDH(pgp::crypto::ecc_curve::ECCCurve::Curve25519))
            .can_sign(false)
            .can_encrypt(true)
            .can_authenticate(false);

        let mut authkey = SubkeyParamsBuilder::default();
        authkey
            .key_type(KeyType::Ed25519Legacy)
            .can_sign(false)
            .can_encrypt(false)
            .can_authenticate(true);

        // Specify the key parameters (todo: check out what matters here)
        let key_params = SecretKeyParamsBuilder::default()
            .created_at(chrono::DateTime::<Utc>::UNIX_EPOCH) // hardcode the earliest timestamp for reproducibility
            .key_type(KeyType::Ed25519Legacy)
            .can_certify(true)
            .can_sign(false)
            .can_encrypt(false)
            .primary_user_id("eigenwallet-pgp".into())
            .subkeys(vec![
                signkey.build().context("failed to build signkey")?,
                encryptkey.build().context("failed to build encryptkey")?,
                authkey.build().context("failed to build authkey")?,
            ])
            .build()
            .context("Failed to build key params")?;

        // Actually derive the secret key
        let secret_key = key_params
            .generate(&mut seeded_rng)
            .context("failed to generate pgp key from monero seed")?;

        // Self sign and finalize the key
        let signed_secret_key = secret_key
            .sign(&mut seeded_rng, &Password::from("")) // No password for now
            .context("couldn't self sign secret key")?;

        Ok(PgpKey {
            inner: signed_secret_key,
        })
    }

    pub fn fingerprint(&self) -> String {
        self.inner.fingerprint().to_string()
    }

    pub fn public_key(&self) -> String {
        self.inner
            .signed_public_key()
            .to_armored_string(ArmorOptions::default())
            .expect("valid key to produce valid string")
    }

    pub fn decrypt(&self, encrypted_message: String) -> Result<String> {
        let reader = BufReader::new(encrypted_message.as_bytes());
        let (mut message, _headers) =
            Message::from_armor(reader).context("couldn't decrypt message")?;

        let mut decrypted_message = String::new();
        message
            .read_to_string(&mut decrypted_message)
            .context("couldn't read valid utf 8 from message")?;

        Ok(decrypted_message)
    }
}
