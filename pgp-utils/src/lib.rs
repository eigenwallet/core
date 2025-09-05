use std::io::{Cursor, Read};
use tracing as _;

use anyhow::{Context, Result, bail};
use chrono::Utc;
use pgp::{
    composed::{
        ArmorOptions, Deserializable, KeyType, Message, SecretKeyParamsBuilder, SignedPublicKey,
        SignedSecretKey, SubkeyParamsBuilder, VerificationResult,
    },
    packet::Signature,
    types::{KeyDetails, Password, PublicKeyTrait},
};
use rand_chacha::rand_core::SeedableRng;

/// Our wrapper around a valid pgp key.
pub struct PgpKey {
    inner: SignedSecretKey,
    /// List of trusted contacts - we only verify messages signed by one of them.
    contacts: Vec<SignedPublicKey>,
}

impl PgpKey {
    /// Try to create a new, random Pgp key with creation time set to unix epoch.
    pub fn new() -> Result<PgpKey> {
        let seed = rand_chacha::ChaCha20Rng::from_entropy().get_seed();
        PgpKey::try_from_seed(seed)
    }

    /// Try to derive a pgp key from a specified seed.
    pub fn try_from_seed(seed_bytes: [u8; 32]) -> Result<PgpKey> {
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
            contacts: Vec::new(),
        })
    }

    /// Add a trusted contact.
    pub fn add_contact(&mut self, public_key: String) -> Result<()> {
        let (public_key, _) = SignedPublicKey::from_armor_single(Cursor::new(public_key))
            .context("Couldn't parse public key")?;

        if !self.contacts.contains(&public_key) {
            self.contacts.push(public_key);
        }

        Ok(())
    }

    /// Get this key's fingerprint.
    pub fn fingerprint(&self) -> String {
        self.inner.fingerprint().to_string()
    }

    /// Get the private key underlying this object.
    pub fn private_key(&self) -> String {
        self.inner
            .to_armored_string(ArmorOptions::default())
            .expect("Valid key to produce valid string")
    }

    /// Get the public key corresponding to this secret key.
    pub fn public_key(&self) -> String {
        self.inner
            .signed_public_key()
            .to_armored_string(ArmorOptions::default())
            .expect("Valid key to produce valid string")
    }

    /// Attempt to decrypt an encrypted message.
    pub fn decrypt(&self, encrypted_message: String) -> Result<String> {
        let cursor = std::io::Cursor::new(encrypted_message);

        let (mut message, _headers) =
            Message::from_armor(cursor).context("Couldn't decode message format")?;

        if message.is_encrypted() {
            message = message
                .decrypt(&Password::empty(), &self.inner)
                .context("Couldn't decrypt message")?;
        }

        if message.is_compressed() {
            message = message
                .decompress()
                .context("Couldn't decompress message")?;
        }

        let mut output = String::new();
        message
            .read_to_string(&mut output)
            .context("Couldn't read decrypted output of message")?;

        Ok(output)
    }

    /// Check whether a message is signed by any of our trusted contacts.
    pub fn verify(&self, signed_message: String) -> Result<bool> {
        if self.contacts.is_empty() {
            bail!("Can't verify message as I have 0 contacts");
        }

        let (message, _) =
            Message::from_armor(Cursor::new(signed_message)).context("couldn't parse message")?;

        let contacts = self
            .contacts
            .iter()
            .map(|c| c as &dyn PublicKeyTrait)
            .collect::<Vec<_>>();
        let results = message.verify_nested(&contacts)?;

        Ok(results
            .iter()
            .any(|i| matches!(i, VerificationResult::Valid(_))))
    }
}

impl From<SignedSecretKey> for PgpKey {
    fn from(value: SignedSecretKey) -> Self {
        Self {
            inner: value,
            contacts: Vec::new(),
        }
    }
}
