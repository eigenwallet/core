use ::bitcoin::bip32::Xpriv as ExtendedPrivKey;
use anyhow::{Context as AnyContext, Result};
use bitcoin::hashes::{sha256, Hash, HashEngine};
use bitcoin::secp256k1::constants::SECRET_KEY_SIZE;
use bitcoin::secp256k1::{self, SecretKey};
use libp2p::identity;
use monero_seed::{Language, Seed as MoneroSeed};
use pem::{encode, Pem};
use rand::prelude::*;
use std::ffi::OsStr;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use swap_fs::ensure_directory_exists;
use zeroize::Zeroizing;

pub const SEED_LENGTH: usize = 32;

#[derive(Clone, Eq, PartialEq)]
pub struct Seed([u8; SEED_LENGTH]);

impl Seed {
    pub fn random() -> Result<Self, Error> {
        let mut bytes = [0u8; SECRET_KEY_SIZE];
        rand::thread_rng().fill_bytes(&mut bytes);

        // If it succeeds once, it'll always succeed
        let _ = SecretKey::from_slice(&bytes)?;

        Ok(Seed(bytes))
    }

    /// Extract seed from a Monero wallet
    pub async fn from_monero_wallet(wallet: &crate::monero::Wallet) -> Result<Self, Error> {
        let mnemonic = wallet.seed().await.context("Failed to get wallet seed")?;

        let monero_seed =
            MoneroSeed::from_string(Language::English, Zeroizing::new(mnemonic.clone())).map_err(
                |e| anyhow::anyhow!("Failed to parse seed from wallet (Error: {:?})", e),
            )?;

        Ok(Seed(*monero_seed.entropy()))
    }

    pub fn derive_extended_private_key(
        &self,
        network: bitcoin::Network,
    ) -> Result<ExtendedPrivKey> {
        let seed = self.derive(b"BITCOIN_EXTENDED_PRIVATE_KEY").bytes();
        let private_key = ExtendedPrivKey::new_master(network, &seed)
            .with_context(|| "Failed to create new master extended private key")?;

        Ok(private_key)
    }

    /// Same as `derive_extended_private_key`, but using the legacy BDK API.
    ///
    /// This is only used for the migration path from the old wallet format to the new one.
    pub fn derive_extended_private_key_legacy(
        &self,
        network: bdk::bitcoin::Network,
    ) -> Result<bdk::bitcoin::util::bip32::ExtendedPrivKey> {
        let seed = self.derive(b"BITCOIN_EXTENDED_PRIVATE_KEY").bytes();
        let private_key = bdk::bitcoin::util::bip32::ExtendedPrivKey::new_master(network, &seed)
            .with_context(|| "Failed to create new master extended private key")?;

        Ok(private_key)
    }

    pub fn derive_libp2p_identity(&self) -> identity::Keypair {
        let bytes = self.derive(b"NETWORK").derive(b"LIBP2P_IDENTITY").bytes();

        identity::Keypair::ed25519_from_bytes(bytes).expect("we always pass 32 bytes")
    }

    /// Create seed from a Monero wallet mnemonic string
    pub fn from_mnemonic(mnemonic: String) -> Result<Self, Error> {
        let monero_seed = MoneroSeed::from_string(Language::English, Zeroizing::new(mnemonic))
            .with_context(|| "Failed to parse mnemonic")?;
        Ok(Seed(*monero_seed.entropy()))
    }

    pub async fn from_file_or_generate(data_dir: &Path) -> Result<Self> {
        let file_path_buf = data_dir.join("seed.pem");
        let file_path = Path::new(&file_path_buf);

        if file_path.exists() {
            return Self::from_file(file_path).with_context(|| "Couldn't get seed from file");
        }

        tracing::debug!("No seed file found, creating at {}", file_path.display());

        let random_seed = Seed::random()?;

        random_seed.write_to(file_path.to_path_buf())?;
        Ok(random_seed)
    }

    /// Derive a new seed using the given scope.
    ///
    /// This function is purposely kept private because it is only a helper
    /// function for deriving specific secret material from the root seed
    /// like the libp2p identity or the seed for the Bitcoin wallet.
    fn derive(&self, scope: &[u8]) -> Self {
        let mut engine = sha256::HashEngine::default();

        engine.input(&self.bytes());
        engine.input(scope);

        let hash = sha256::Hash::from_engine(engine);

        Self(hash.to_byte_array())
    }

    fn bytes(&self) -> [u8; SEED_LENGTH] {
        self.0
    }

    fn from_file<D>(seed_file: D) -> Result<Self, Error>
    where
        D: AsRef<OsStr>,
    {
        let file = Path::new(&seed_file);
        let contents = fs::read_to_string(file)?;
        let pem = pem::parse(contents)?;

        tracing::debug!("Reading in seed from {}", file.display());

        Self::from_pem(pem)
    }

    fn from_pem(pem: pem::Pem) -> Result<Self, Error> {
        let contents = pem.contents();
        if contents.len() != SEED_LENGTH {
            Err(Error::IncorrectLength(contents.len()))
        } else {
            let mut array = [0; SEED_LENGTH];
            for (i, b) in contents.iter().enumerate() {
                array[i] = *b;
            }

            Ok(Self::from(array))
        }
    }

    fn write_to(&self, seed_file: PathBuf) -> Result<(), Error> {
        ensure_directory_exists(&seed_file)?;

        let data = self.bytes();
        let pem = Pem::new("SEED", data);

        let pem_string = encode(&pem);

        let mut file = File::create(seed_file)?;
        file.write_all(pem_string.as_bytes())?;

        Ok(())
    }
}

impl fmt::Debug for Seed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Seed([*****])")
    }
}

impl fmt::Display for Seed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl From<[u8; SEED_LENGTH]> for Seed {
    fn from(bytes: [u8; SEED_LENGTH]) -> Self {
        Seed(bytes)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Secp256k1: ")]
    Secp256k1(#[from] secp256k1::Error),
    #[error("io: ")]
    Io(#[from] io::Error),
    #[error("PEM parse: ")]
    PemParse(#[from] pem::PemError),
    #[error("expected 32 bytes of base64 encode, got {0} bytes")]
    IncorrectLength(usize),
    #[error("RNG: ")]
    Rand(#[from] rand::Error),
    #[error("no default path")]
    NoDefaultPath,
    #[error("Monero wallet error: {0}")]
    MoneroWallet(#[from] anyhow::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;

    #[test]
    fn generate_random_seed() {
        let _ = Seed::random().unwrap();
    }

    #[test]
    fn seed_byte_string_must_be_32_bytes_long() {
        let _seed = Seed::from(*b"this string is exactly 32 bytes!");
    }

    #[test]
    fn seed_from_pem_works() {
        use base64::engine::general_purpose;
        use base64::Engine;

        let payload: &str = "syl9wSYaruvgxg9P5Q1qkZaq5YkM6GvXkxe+VYrL/XM=";

        // 32 bytes base64 encoded.
        let pem_string: &str = "-----BEGIN SEED-----
syl9wSYaruvgxg9P5Q1qkZaq5YkM6GvXkxe+VYrL/XM=
-----END SEED-----
";

        let want = general_purpose::STANDARD.decode(payload).unwrap();
        let pem = pem::parse(pem_string).unwrap();
        let got = Seed::from_pem(pem).unwrap();

        assert_eq!(got.bytes(), *want);
    }

    #[test]
    fn seed_from_pem_fails_for_short_seed() {
        let short = "-----BEGIN SEED-----
VnZUNFZ4dlY=
-----END SEED-----
";
        let pem = pem::parse(short).unwrap();
        match Seed::from_pem(pem) {
            Ok(_) => panic!("should fail for short payload"),
            Err(e) => {
                match e {
                    Error::IncorrectLength(_) => {} // pass
                    _ => panic!("should fail with IncorrectLength error"),
                }
            }
        }
    }

    #[test]
    fn seed_from_pem_fails_for_long_seed() {
        let long = "-----BEGIN SEED-----
MIIBPQIBAAJBAOsfi5AGYhdRs/x6q5H7kScxA0Kzzqe6WI6gf6+tc6IvKQJo5rQc
dWWSQ0nRGt2hOPDO+35NKhQEjBQxPh/v7n0CAwEAAQJBAOGaBAyuw0ICyENy5NsO
-----END SEED-----
";
        let pem = pem::parse(long).unwrap();
        assert_eq!(pem.contents().len(), 96);

        match Seed::from_pem(pem) {
            Ok(_) => panic!("should fail for long payload"),
            Err(e) => {
                match e {
                    Error::IncorrectLength(len) => assert_eq!(len, 96), // pass
                    _ => panic!("should fail with IncorrectLength error"),
                }
            }
        }
    }

    #[test]
    fn round_trip_through_file_write_read() {
        let tmpfile = temp_dir().join("seed.pem");

        let seed = Seed::random().unwrap();
        seed.write_to(tmpfile.clone())
            .expect("Write seed to temp file");

        let rinsed = Seed::from_file(tmpfile).expect("Read from temp file");
        assert_eq!(seed.0, rinsed.0);
    }
}
