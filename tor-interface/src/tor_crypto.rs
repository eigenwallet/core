// standard
use std::convert::TryInto;
use std::str;

// extern crates
use crypto::digest::Digest;
use crypto::sha1::Sha1;
use crypto::sha3::Sha3;
use data_encoding::{BASE32, BASE32_NOPAD, BASE64, HEXUPPER};
use data_encoding_macro::new_encoding;
use rand::rngs::OsRng;
use rand::RngCore;
use signature::Verifier;
use tor_llcrypto::pk::keymanip::*;
use tor_llcrypto::util::rand_compat::RngCompatExt;
use tor_llcrypto::*;

// internal modules
use crate::error::TorCryptoError;

/// The number of bytes in an ed25519 secret key
/// cbindgen:ignore
pub const ED25519_PRIVATE_KEY_SIZE: usize = 64;
/// The number of bytes in an ed25519 public key
/// cbindgen:ignore
pub const ED25519_PUBLIC_KEY_SIZE: usize = 32;
/// The number of bytes in an ed25519 signature
/// cbindgen:ignore
pub const ED25519_SIGNATURE_SIZE: usize = 64;
/// The number of bytes needed to store onion service id as an ASCII c-string (not including null-terminator)
pub const V3_ONION_SERVICE_ID_LENGTH: usize = 56;
/// The number of bytes needed to store onion service id as an ASCII c-string (including null-terminator)
pub const V3_ONION_SERVICE_ID_SIZE: usize = V3_ONION_SERVICE_ID_LENGTH + 1;
/// The number of bytes needed to store base64 encoded ed25519 private key as an ASCII c-string (not including null-terminator)
pub const ED25519_PRIVATE_KEYBLOB_BASE64_LENGTH: usize = 88;
/// key klob header string
const ED25519_PRIVATE_KEYBLOB_HEADER: &str = "ED25519-V3:";
/// The number of bytes needed to store the keyblob header
pub const ED25519_PRIVATE_KEYBLOB_HEADER_LENGTH: usize = 11;
/// The number of bytes needed to store ed25519 private keyblob as an ASCII c-string (not including a null terminator)
pub const ED25519_PRIVATE_KEYBLOB_LENGTH: usize =
    ED25519_PRIVATE_KEYBLOB_HEADER_LENGTH + ED25519_PRIVATE_KEYBLOB_BASE64_LENGTH;
/// The number of bytes needed to store ed25519 private keyblob as an ASCII c-string (including a null terminator)
pub const ED25519_PRIVATE_KEYBLOB_SIZE: usize = ED25519_PRIVATE_KEYBLOB_LENGTH + 1;
// number of bytes in an onion service id after base32 decode
const V3_ONION_SERVICE_ID_RAW_SIZE: usize = 35;
// byte index of the start of the public key checksum
const V3_ONION_SERVICE_ID_CHECKSUM_OFFSET: usize = 32;
// byte index of the v3 onion service version
const V3_ONION_SERVICE_ID_VERSION_OFFSET: usize = 34;
/// The number of bytes in a v3 service id's truncated checksum
const TRUNCATED_CHECKSUM_SIZE: usize = 2;
/// The number of bytes in an x25519 private key
/// cbindgen:ignore
pub const X25519_PRIVATE_KEY_SIZE: usize = 32;
/// The number of bytes in an x25519 publickey
/// cbindgen:ignore
pub const X25519_PUBLIC_KEY_SIZE: usize = 32;
/// The number of bytes needed to store base64 encoded x25519 private key as an ASCII c-string (not including null-terminator)
pub const X25519_PRIVATE_KEYBLOB_BASE64_LENGTH: usize = 44;
/// The number of bytes needed to store base64 encoded x25519 private key as an ASCII c-string (including a null terminator)
pub const X25519_PRIVATE_KEYBLOB_BASE64_SIZE: usize = X25519_PRIVATE_KEYBLOB_BASE64_LENGTH + 1;
/// The number of bytes needed to store base32 encoded x25519 public key as an ASCII c-string (not including null-terminator)
pub const X25519_PUBLIC_KEYBLOB_BASE32_LENGTH: usize = 52;
/// The number of bytes needed to store bsae32 encoded x25519 public key as an ASCII c-string (including a null terminator)
pub const X25519_PUBLIC_KEYBLOB_BASE32_SIZE: usize = X25519_PUBLIC_KEYBLOB_BASE32_LENGTH + 1;

const ONION_BASE32: data_encoding::Encoding = new_encoding! {
    symbols: "abcdefghijklmnopqrstuvwxyz234567",
    padding: '=',
};

const SHA1_BYTES: usize = 160 / 8;
const S2K_RFC2440_SPECIFIER_LEN: usize = 9;

// see https://github.com/torproject/torspec/blob/main/rend-spec-v3.txt#L2143
fn calc_truncated_checksum(
    public_key: &[u8; ED25519_PUBLIC_KEY_SIZE],
) -> [u8; TRUNCATED_CHECKSUM_SIZE] {
    // space for full checksum
    const SHA256_BYTES: usize = 256 / 8;
    let mut hash_bytes = [0u8; SHA256_BYTES];

    let mut hasher = Sha3::sha3_256();
    assert_eq!(SHA256_BYTES, hasher.output_bytes());

    // calculate checksum
    hasher.input(b".onion checksum");
    hasher.input(public_key);
    hasher.input(&[0x03u8]);
    hasher.result(&mut hash_bytes);

    [hash_bytes[0], hash_bytes[1]]
}

// Free functions

fn hash_tor_password_with_salt(salt: &[u8; S2K_RFC2440_SPECIFIER_LEN], password: &str) -> String {
    assert!(salt[S2K_RFC2440_SPECIFIER_LEN - 1] == 0x60);

    // tor-specific rfc 2440 constants
    const EXPBIAS: u8 = 6u8;
    const C: u8 = 0x60; // salt[S2K_RFC2440_SPECIFIER_LEN - 1]
    const COUNT: usize = (16usize + ((C & 15u8) as usize)) << ((C >> 4) + EXPBIAS);

    // squash together our hash input
    let mut input: Vec<u8> = Default::default();
    // append salt (sans the 'C' constant')
    input.extend_from_slice(&salt[0..S2K_RFC2440_SPECIFIER_LEN - 1]);
    // append password bytes
    input.extend_from_slice(password.as_bytes());

    let input = input.as_slice();
    let input_len = input.len();

    let mut sha1 = Sha1::new();
    let mut count = COUNT;
    while count > 0 {
        if count > input_len {
            sha1.input(input);
            count -= input_len;
        } else {
            sha1.input(&input[0..count]);
            break;
        }
    }

    let mut key = [0u8; SHA1_BYTES];
    sha1.result(key.as_mut_slice());

    let mut hash = "16:".to_string();
    HEXUPPER.encode_append(salt, &mut hash);
    HEXUPPER.encode_append(&key, &mut hash);

    hash
}

pub fn hash_tor_password(password: &str) -> String {
    let mut salt = [0x00u8; S2K_RFC2440_SPECIFIER_LEN];
    OsRng.fill_bytes(&mut salt);
    salt[S2K_RFC2440_SPECIFIER_LEN - 1] = 0x60u8;

    hash_tor_password_with_salt(&salt, password)
}

// Struct deinitions

pub struct Ed25519PrivateKey {
    expanded_secret_key: pk::ed25519::ExpandedSecretKey,
}

#[derive(Clone)]
pub struct Ed25519PublicKey {
    public_key: pk::ed25519::PublicKey,
}

#[derive(Clone)]
pub struct Ed25519Signature {
    signature: pk::ed25519::Signature,
}

#[derive(Clone)]
pub struct X25519PrivateKey {
    secret_key: pk::curve25519::StaticSecret,
}

#[derive(Clone)]
pub struct X25519PublicKey {
    public_key: pk::curve25519::PublicKey,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct V3OnionServiceId {
    data: [u8; V3_ONION_SERVICE_ID_LENGTH],
}

#[derive(Clone, Copy)]
pub enum SignBit {
    Zero,
    One,
}

impl From<SignBit> for u8 {
    fn from(signbit: SignBit) -> Self {
        match signbit {
            SignBit::Zero => 0u8,
            SignBit::One => 1u8,
        }
    }
}

impl From<SignBit> for bool {
    fn from(signbit: SignBit) -> Self {
        match signbit {
            SignBit::Zero => false,
            SignBit::One => true,
        }
    }
}

impl From<bool> for SignBit {
    fn from(signbit: bool) -> Self {
        if signbit {
            SignBit::One
        } else {
            SignBit::Zero
        }
    }
}

// Ed25519 Private Key

impl Ed25519PrivateKey {
    pub fn generate() -> Ed25519PrivateKey {
        let secret_key = pk::ed25519::SecretKey::generate(&mut rand_core::OsRng.rng_compat());

        Ed25519PrivateKey {
            expanded_secret_key: pk::ed25519::ExpandedSecretKey::from(&secret_key),
        }
    }

    // according to nickm, any 64 byte string here is allowed
    pub fn from_raw(raw: &[u8; ED25519_PRIVATE_KEY_SIZE]) -> Ed25519PrivateKey {
        Ed25519PrivateKey {
            expanded_secret_key: match pk::ed25519::ExpandedSecretKey::from_bytes(raw) {
                Ok(expanded_secret_key) => expanded_secret_key,
                Err(_) => unreachable!(),
            },
        }
    }

    pub fn from_key_blob(key_blob: &str) -> Result<Ed25519PrivateKey, TorCryptoError> {
        if key_blob.len() != ED25519_PRIVATE_KEYBLOB_LENGTH {
            return Err(TorCryptoError::ParseError(format!(
                "expects string of length '{}'; received string with length '{}'",
                ED25519_PRIVATE_KEYBLOB_LENGTH,
                key_blob.len()
            )));
        }

        if !key_blob.starts_with(ED25519_PRIVATE_KEYBLOB_HEADER) {
            return Err(TorCryptoError::ParseError(format!(
                "expects string that begins with '{}'; received '{}'",
                &ED25519_PRIVATE_KEYBLOB_HEADER, &key_blob
            )));
        }

        let base64_key: &str = &key_blob[ED25519_PRIVATE_KEYBLOB_HEADER.len()..];
        let private_key_data = match BASE64.decode(base64_key.as_bytes()) {
            Ok(private_key_data) => private_key_data,
            Err(_) => {
                return Err(TorCryptoError::ParseError(format!(
                    "could not parse '{}' as base64",
                    base64_key
                )))
            }
        };
        let private_key_data_len = private_key_data.len();
        let private_key_data_raw: [u8; ED25519_PRIVATE_KEY_SIZE] = match private_key_data.try_into()
        {
            Ok(private_key_data) => private_key_data,
            Err(_) => {
                return Err(TorCryptoError::ParseError(format!(
                    "expects decoded private key length '{}'; actual '{}'",
                    ED25519_PRIVATE_KEY_SIZE, private_key_data_len
                )))
            }
        };

        Ok(Ed25519PrivateKey::from_raw(&private_key_data_raw))
    }

    pub fn from_private_x25519(
        x25519_private: &X25519PrivateKey,
    ) -> Result<(Ed25519PrivateKey, SignBit), TorCryptoError> {
        if let Some((result, signbit)) =
            convert_curve25519_to_ed25519_private(&x25519_private.secret_key)
        {
            Ok((
                Ed25519PrivateKey {
                    expanded_secret_key: result,
                },
                match signbit {
                    0u8 => SignBit::Zero,
                    1u8 => SignBit::One,
                    invalid_signbit => {
                        return Err(TorCryptoError::ConversionError(format!(
                            "convert_curve25519_to_ed25519_private() returned invalid signbit: {}",
                            invalid_signbit
                        )))
                    }
                },
            ))
        } else {
            Err(TorCryptoError::ConversionError(
                "could not convert x25519 private key to ed25519 private key".to_string(),
            ))
        }
    }

    pub fn to_key_blob(&self) -> String {
        let mut key_blob = ED25519_PRIVATE_KEYBLOB_HEADER.to_string();
        key_blob.push_str(&BASE64.encode(&self.expanded_secret_key.to_bytes()));

        key_blob
    }

    pub fn sign_message_ex(
        &self,
        public_key: &Ed25519PublicKey,
        message: &[u8],
    ) -> Ed25519Signature {
        let signature = self
            .expanded_secret_key
            .sign(message, &public_key.public_key);
        Ed25519Signature { signature }
    }

    pub fn sign_message(&self, message: &[u8]) -> Ed25519Signature {
        let public_key = Ed25519PublicKey::from_private_key(self);
        self.sign_message_ex(&public_key, message)
    }

    pub fn to_bytes(&self) -> [u8; ED25519_PRIVATE_KEY_SIZE] {
        self.expanded_secret_key.to_bytes()
    }
}

impl PartialEq for Ed25519PrivateKey {
    fn eq(&self, other: &Self) -> bool {
        self.to_bytes().eq(&other.to_bytes())
    }
}

impl Clone for Ed25519PrivateKey {
    fn clone(&self) -> Ed25519PrivateKey {
        Ed25519PrivateKey::from_raw(&self.to_bytes())
    }
}

impl std::fmt::Debug for Ed25519PrivateKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "--- ed25519 private key ---")
    }
}

// Ed25519 Public Key

impl Ed25519PublicKey {
    pub fn from_raw(
        raw: &[u8; ED25519_PUBLIC_KEY_SIZE],
    ) -> Result<Ed25519PublicKey, TorCryptoError> {
        Ok(Ed25519PublicKey {
            public_key: match pk::ed25519::PublicKey::from_bytes(raw) {
                Ok(public_key) => public_key,
                Err(_) => {
                    return Err(TorCryptoError::ConversionError(
                        "failed to create ed25519 public key from bytes".to_string(),
                    ))
                }
            },
        })
    }

    pub fn from_service_id(
        service_id: &V3OnionServiceId,
    ) -> Result<Ed25519PublicKey, TorCryptoError> {
        // decode base32 encoded service id
        let mut decoded_service_id = [0u8; V3_ONION_SERVICE_ID_RAW_SIZE];
        let decoded_byte_count =
            match ONION_BASE32.decode_mut(service_id.as_bytes(), &mut decoded_service_id) {
                Ok(decoded_byte_count) => decoded_byte_count,
                Err(_) => {
                    return Err(TorCryptoError::ConversionError(format!(
                        "failed to decode '{}' as V3OnionServiceId",
                        service_id.to_string()
                    )))
                }
            };
        if decoded_byte_count != V3_ONION_SERVICE_ID_RAW_SIZE {
            return Err(TorCryptoError::ConversionError(format!(
                "decoded byte count is '{}', expected '{}'",
                decoded_byte_count, V3_ONION_SERVICE_ID_RAW_SIZE
            )));
        }

        Ed25519PublicKey::from_raw(
            decoded_service_id[0..ED25519_PUBLIC_KEY_SIZE]
                .try_into()
                .unwrap(),
        )
    }

    pub fn from_private_key(private_key: &Ed25519PrivateKey) -> Ed25519PublicKey {
        Ed25519PublicKey {
            public_key: pk::ed25519::PublicKey::from(&private_key.expanded_secret_key),
        }
    }

    fn from_public_x25519(
        public_x25519: &X25519PublicKey,
        signbit: SignBit,
    ) -> Result<Ed25519PublicKey, TorCryptoError> {
        match convert_curve25519_to_ed25519_public(&public_x25519.public_key, signbit.into()) {
            Some(public_key) => Ok(Ed25519PublicKey { public_key }),
            None => Err(TorCryptoError::ConversionError(
                "failed to create ed25519 public key from x25519 public key and signbit"
                    .to_string(),
            )),
        }
    }

    pub fn to_base32(&self) -> String {
        BASE32.encode(self.as_bytes())
    }

    pub fn as_bytes(&self) -> &[u8; ED25519_PUBLIC_KEY_SIZE] {
        self.public_key.as_bytes()
    }
}

impl PartialEq for Ed25519PublicKey {
    fn eq(&self, other: &Self) -> bool {
        self.public_key.eq(&other.public_key)
    }
}

// Ed25519 Signature

impl Ed25519Signature {
    pub fn from_raw(
        raw: &[u8; ED25519_SIGNATURE_SIZE],
    ) -> Result<Ed25519Signature, TorCryptoError> {
        Ok(Ed25519Signature {
            signature: match pk::ed25519::Signature::from_bytes(raw) {
                Ok(signature) => signature,
                Err(_) => {
                    return Err(TorCryptoError::ConversionError(
                        "failed to create ed25519 signature from bytes".to_string(),
                    ))
                }
            },
        })
    }

    pub fn verify(&self, message: &[u8], public_key: &Ed25519PublicKey) -> bool {
        if let Ok(()) = public_key.public_key.verify(message, &self.signature) {
            return true;
        }
        false
    }

    // derives an ed25519 public key from the provided x25519 public key and signbit, then
    // verifies this signature using said ed25519 public key
    pub fn verify_x25519(
        &self,
        message: &[u8],
        public_key: &X25519PublicKey,
        signbit: SignBit,
    ) -> bool {
        if let Ok(public_key) = Ed25519PublicKey::from_public_x25519(public_key, signbit) {
            return self.verify(message, &public_key);
        }
        false
    }

    pub fn to_bytes(&self) -> [u8; ED25519_SIGNATURE_SIZE] {
        self.signature.to_bytes()
    }
}

impl PartialEq for Ed25519Signature {
    fn eq(&self, other: &Self) -> bool {
        self.signature.eq(&other.signature)
    }
}

// X25519 Private Key

impl X25519PrivateKey {
    pub fn generate() -> X25519PrivateKey {
        X25519PrivateKey {
            secret_key: pk::curve25519::StaticSecret::new(rand_core::OsRng.rng_compat()),
        }
    }

    pub fn from_raw(raw: &[u8; X25519_PRIVATE_KEY_SIZE]) -> X25519PrivateKey {
        X25519PrivateKey {
            secret_key: pk::curve25519::StaticSecret::from(*raw),
        }
    }

    // a base64 encoded keyblob
    pub fn from_base64(base64: &str) -> Result<X25519PrivateKey, TorCryptoError> {
        if base64.len() != X25519_PRIVATE_KEYBLOB_BASE64_LENGTH {
            return Err(TorCryptoError::ParseError(format!(
                "expects string of length '{}'; received string with length '{}'",
                X25519_PRIVATE_KEYBLOB_BASE64_LENGTH,
                base64.len()
            )));
        }

        let private_key_data = match BASE64.decode(base64.as_bytes()) {
            Ok(private_key_data) => private_key_data,
            Err(_) => {
                return Err(TorCryptoError::ParseError(format!(
                    "could not parse '{}' as base64",
                    base64
                )))
            }
        };
        let private_key_data_len = private_key_data.len();
        let private_key_data_raw: [u8; X25519_PRIVATE_KEY_SIZE] = match private_key_data.try_into()
        {
            Ok(private_key_data) => private_key_data,
            Err(_) => {
                return Err(TorCryptoError::ParseError(format!(
                    "expects decoded private key length '{}'; actual '{}'",
                    X25519_PRIVATE_KEY_SIZE, private_key_data_len
                )))
            }
        };

        Ok(X25519PrivateKey::from_raw(&private_key_data_raw))
    }

    // security note: only ever sign messages the private key owner controls the contents of!
    // this function first derives an ed25519 private key from the provided x25519 private key
    // and signs the message, returning the signature and signbit needed to calculate the
    // ed25519 public key from our x25519 private key's associated x25519 public key
    pub fn sign_message(
        &self,
        message: &[u8],
    ) -> Result<(Ed25519Signature, SignBit), TorCryptoError> {
        let (ed25519_private, signbit) = Ed25519PrivateKey::from_private_x25519(self)?;
        Ok((ed25519_private.sign_message(message), signbit))
    }

    pub fn to_base64(&self) -> String {
        BASE64.encode(&self.secret_key.to_bytes())
    }

    pub fn to_bytes(&self) -> [u8; X25519_PRIVATE_KEY_SIZE] {
        self.secret_key.to_bytes()
    }
}

impl std::fmt::Debug for X25519PrivateKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "--- x25519 private key ---")
    }
}

// X25519 Public Key
impl X25519PublicKey {
    pub fn from_private_key(private_key: &X25519PrivateKey) -> X25519PublicKey {
        X25519PublicKey {
            public_key: pk::curve25519::PublicKey::from(&private_key.secret_key),
        }
    }

    pub fn from_raw(raw: &[u8; X25519_PUBLIC_KEY_SIZE]) -> X25519PublicKey {
        X25519PublicKey {
            public_key: pk::curve25519::PublicKey::from(*raw),
        }
    }

    pub fn from_base32(base32: &str) -> Result<X25519PublicKey, TorCryptoError> {
        if base32.len() != X25519_PUBLIC_KEYBLOB_BASE32_LENGTH {
            return Err(TorCryptoError::ParseError(format!(
                "expects string of length '{}'; received '{}' with length '{}'",
                X25519_PUBLIC_KEYBLOB_BASE32_LENGTH,
                base32,
                base32.len()
            )));
        }

        let public_key_data = match BASE32_NOPAD.decode(base32.as_bytes()) {
            Ok(public_key_data) => public_key_data,
            Err(_) => {
                return Err(TorCryptoError::ParseError(format!(
                    "failed to decode '{}' as X25519PublicKey",
                    base32
                )))
            }
        };
        let public_key_data_len = public_key_data.len();
        let public_key_data_raw: [u8; X25519_PUBLIC_KEY_SIZE] = match public_key_data.try_into() {
            Ok(public_key_data) => public_key_data,
            Err(_) => {
                return Err(TorCryptoError::ParseError(format!(
                    "expects decoded public key length '{}'; actual '{}'",
                    X25519_PUBLIC_KEY_SIZE, public_key_data_len
                )))
            }
        };

        Ok(X25519PublicKey::from_raw(&public_key_data_raw))
    }

    pub fn to_base32(&self) -> String {
        BASE32_NOPAD.encode(self.public_key.as_bytes())
    }

    pub fn to_string(&self) -> String {
        self.to_base32()
    }

    pub fn as_bytes(&self) -> &[u8; X25519_PUBLIC_KEY_SIZE] {
        self.public_key.as_bytes()
    }
}

impl std::fmt::Debug for X25519PublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_string())
    }
}

// Onion Service Id

impl V3OnionServiceId {
    pub fn from_string(service_id: &str) -> Result<V3OnionServiceId, TorCryptoError> {
        if !V3OnionServiceId::is_valid(service_id) {
            return Err(TorCryptoError::ParseError(format!(
                "'{}' is not a valid v3 onion service id",
                service_id
            )));
        }
        Ok(V3OnionServiceId {
            data: service_id.as_bytes().try_into().unwrap(),
        })
    }

    pub fn from_public_key(public_key: &Ed25519PublicKey) -> V3OnionServiceId {
        let mut raw_service_id = [0u8; V3_ONION_SERVICE_ID_RAW_SIZE];

        raw_service_id[..ED25519_PUBLIC_KEY_SIZE].copy_from_slice(&public_key.as_bytes()[..]);
        let truncated_checksum = calc_truncated_checksum(public_key.as_bytes());
        raw_service_id[V3_ONION_SERVICE_ID_CHECKSUM_OFFSET] = truncated_checksum[0];
        raw_service_id[V3_ONION_SERVICE_ID_CHECKSUM_OFFSET + 1] = truncated_checksum[1];
        raw_service_id[V3_ONION_SERVICE_ID_VERSION_OFFSET] = 0x03u8;

        let mut service_id = [0u8; V3_ONION_SERVICE_ID_LENGTH];
        // panics on wrong buffer size, but given our constant buffer sizes should be fine
        ONION_BASE32.encode_mut(&raw_service_id, &mut service_id);

        V3OnionServiceId { data: service_id }
    }

    pub fn from_private_key(private_key: &Ed25519PrivateKey) -> V3OnionServiceId {
        Self::from_public_key(&Ed25519PublicKey::from_private_key(private_key))
    }

    pub fn is_valid(service_id: &str) -> bool {
        if service_id.len() != V3_ONION_SERVICE_ID_LENGTH {
            return false;
        }

        let mut decoded_service_id = [0u8; V3_ONION_SERVICE_ID_RAW_SIZE];
        match ONION_BASE32.decode_mut(service_id.as_bytes(), &mut decoded_service_id) {
            Ok(decoded_byte_count) => {
                // ensure right size
                if decoded_byte_count != V3_ONION_SERVICE_ID_RAW_SIZE {
                    return false;
                }
                // ensure correct version
                if decoded_service_id[V3_ONION_SERVICE_ID_VERSION_OFFSET] != 0x03 {
                    return false;
                }
                // copy public key into own buffer
                let mut public_key = [0u8; ED25519_PUBLIC_KEY_SIZE];
                public_key[..].copy_from_slice(&decoded_service_id[..ED25519_PUBLIC_KEY_SIZE]);
                // ensure checksum is correct
                let truncated_checksum = calc_truncated_checksum(&public_key);
                if truncated_checksum[0] != decoded_service_id[V3_ONION_SERVICE_ID_CHECKSUM_OFFSET]
                    || truncated_checksum[1]
                        != decoded_service_id[V3_ONION_SERVICE_ID_CHECKSUM_OFFSET + 1]
                {
                    return false;
                }
                true
            }
            Err(_) => false,
        }
    }

    pub fn as_bytes(&self) -> &[u8; V3_ONION_SERVICE_ID_LENGTH] {
        &self.data
    }
}

impl ToString for V3OnionServiceId {
    fn to_string(&self) -> String {
        return unsafe { str::from_utf8_unchecked(&self.data).to_string() };
    }
}

impl std::fmt::Debug for V3OnionServiceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_string())
    }
}

#[test]
fn test_ed25519() -> Result<(), anyhow::Error> {
    let private_key_blob = "ED25519-V3:YE3GZtDmc+izGijWKgeVRabbXqK456JKKGONDBhV+kPBVKa2mHVQqnRTVuFXe3inU3YW6qvc7glYEwe9rK0LhQ==";
    let private_raw: [u8; ED25519_PRIVATE_KEY_SIZE] = [
        0x60u8, 0x4du8, 0xc6u8, 0x66u8, 0xd0u8, 0xe6u8, 0x73u8, 0xe8u8, 0xb3u8, 0x1au8, 0x28u8,
        0xd6u8, 0x2au8, 0x07u8, 0x95u8, 0x45u8, 0xa6u8, 0xdbu8, 0x5eu8, 0xa2u8, 0xb8u8, 0xe7u8,
        0xa2u8, 0x4au8, 0x28u8, 0x63u8, 0x8du8, 0x0cu8, 0x18u8, 0x55u8, 0xfau8, 0x43u8, 0xc1u8,
        0x54u8, 0xa6u8, 0xb6u8, 0x98u8, 0x75u8, 0x50u8, 0xaau8, 0x74u8, 0x53u8, 0x56u8, 0xe1u8,
        0x57u8, 0x7bu8, 0x78u8, 0xa7u8, 0x53u8, 0x76u8, 0x16u8, 0xeau8, 0xabu8, 0xdcu8, 0xeeu8,
        0x09u8, 0x58u8, 0x13u8, 0x07u8, 0xbdu8, 0xacu8, 0xadu8, 0x0bu8, 0x85u8,
    ];
    let public_raw: [u8; ED25519_PUBLIC_KEY_SIZE] = [
        0xf2u8, 0xfdu8, 0xa2u8, 0xdbu8, 0xf3u8, 0x80u8, 0xa6u8, 0xbau8, 0x74u8, 0xa4u8, 0x90u8,
        0xe1u8, 0x45u8, 0x55u8, 0xeeu8, 0xb9u8, 0x32u8, 0xa0u8, 0x5cu8, 0x39u8, 0x5au8, 0xe2u8,
        0x02u8, 0x83u8, 0x55u8, 0x27u8, 0x89u8, 0x6au8, 0x1fu8, 0x2fu8, 0x3du8, 0xc5u8,
    ];
    let public_base32 = "6L62FW7TQCTLU5FESDQUKVPOXEZKAXBZLLRAFA2VE6EWUHZPHXCQ====";
    let service_id_string = "6l62fw7tqctlu5fesdqukvpoxezkaxbzllrafa2ve6ewuhzphxczsjyd";
    assert!(V3OnionServiceId::is_valid(&service_id_string));

    let mut message = [0x00u8; 256];
    let null_message = [0x00u8; 256];
    for (i, ptr) in message.iter_mut().enumerate() {
        *ptr = i as u8;
    }
    let signature_raw: [u8; ED25519_SIGNATURE_SIZE] = [
        0xa6u8, 0xd6u8, 0xc6u8, 0x1au8, 0x03u8, 0xbcu8, 0x43u8, 0x6fu8, 0x38u8, 0x53u8, 0x94u8,
        0xcdu8, 0xdcu8, 0x86u8, 0x0au8, 0x88u8, 0x64u8, 0x43u8, 0x1du8, 0x18u8, 0x84u8, 0x30u8,
        0x2fu8, 0xcdu8, 0xa6u8, 0x79u8, 0xcau8, 0x87u8, 0xd0u8, 0x29u8, 0xe7u8, 0x2bu8, 0x32u8,
        0x9bu8, 0xa2u8, 0xa4u8, 0x3cu8, 0x74u8, 0x6au8, 0x08u8, 0x67u8, 0x0eu8, 0x63u8, 0x60u8,
        0xcbu8, 0x46u8, 0x22u8, 0x55u8, 0x43u8, 0x5bu8, 0x84u8, 0x68u8, 0x0fu8, 0x47u8, 0xceu8,
        0x6cu8, 0xd2u8, 0xb8u8, 0xebu8, 0xfeu8, 0xf6u8, 0x9eu8, 0x97u8, 0x0au8,
    ];

    // test the golden path first
    let service_id = V3OnionServiceId::from_string(&service_id_string)?;

    let private_key = Ed25519PrivateKey::from_raw(&private_raw);
    assert!(private_key == Ed25519PrivateKey::from_key_blob(&private_key_blob)?);
    assert!(private_key_blob == private_key.to_key_blob());

    let public_key = Ed25519PublicKey::from_raw(&public_raw)?;
    assert!(public_key == Ed25519PublicKey::from_service_id(&service_id)?);
    assert!(public_key == Ed25519PublicKey::from_private_key(&private_key));
    assert!(service_id == V3OnionServiceId::from_public_key(&public_key));
    assert!(public_base32 == public_key.to_base32());

    let signature = private_key.sign_message(&message);
    assert!(signature == Ed25519Signature::from_raw(&signature_raw)?);
    assert!(signature.verify(&message, &public_key));
    assert!(!signature.verify(&null_message, &public_key));

    // some invalid service ids
    assert!(!V3OnionServiceId::is_valid(""));
    assert!(!V3OnionServiceId::is_valid(
        "
        aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    ));
    assert!(!V3OnionServiceId::is_valid(
        "6L62FW7TQCTLU5FESDQUKVPOXEZKAXBZLLRAFA2VE6EWUHZPHXCZSJYD"
    ));

    // generate a new key, get the public key and sign/verify a message
    let private_key = Ed25519PrivateKey::generate();
    let public_key = Ed25519PublicKey::from_private_key(&private_key);
    let signature = private_key.sign_message(&message);
    assert!(signature.verify(&message, &public_key));

    Ok(())
}

#[test]
fn test_password_hash() -> Result<(), anyhow::Error> {
    let salt1: [u8; S2K_RFC2440_SPECIFIER_LEN] = [
        0xbeu8, 0x2au8, 0x25u8, 0x1du8, 0xe6u8, 0x2cu8, 0xb2u8, 0x7au8, 0x60u8,
    ];
    let hash1 = hash_tor_password_with_salt(&salt1, "abcdefghijklmnopqrstuvwxyz");
    assert!(hash1 == "16:BE2A251DE62CB27A60AC9178A937990E8ED0AB662FA82A5C7DE3EBB23A");

    let salt2: [u8; S2K_RFC2440_SPECIFIER_LEN] = [
        0x36u8, 0x73u8, 0x0eu8, 0xefu8, 0xd1u8, 0x8cu8, 0x60u8, 0xd6u8, 0x60u8,
    ];
    let hash2 = hash_tor_password_with_salt(&salt2, "password");
    assert!(hash2 == "16:36730EEFD18C60D66052E7EA535438761C0928D316EEA56A190C99B50A");

    // ensure same password is hashed to different things
    assert!(hash_tor_password("password") != hash_tor_password("password"));

    Ok(())
}

#[test]
fn test_x25519() -> Result<(), anyhow::Error> {
    // private/public key pair
    const SECRET_BASE64: &str = "0GeSReJXdNcgvWRQdnDXhJGdu5UiwP2fefgT93/oqn0=";
    const SECRET_RAW: [u8; X25519_PRIVATE_KEY_SIZE] = [
        0xd0u8, 0x67u8, 0x92u8, 0x45u8, 0xe2u8, 0x57u8, 0x74u8, 0xd7u8, 0x20u8, 0xbdu8, 0x64u8,
        0x50u8, 0x76u8, 0x70u8, 0xd7u8, 0x84u8, 0x91u8, 0x9du8, 0xbbu8, 0x95u8, 0x22u8, 0xc0u8,
        0xfdu8, 0x9fu8, 0x79u8, 0xf8u8, 0x13u8, 0xf7u8, 0x7fu8, 0xe8u8, 0xaau8, 0x7du8,
    ];
    const PUBLIC_BASE32: &str = "AEXCBCEDJ5KU34YGGMZ7PVHVDEA7D7YB7VQAPJTMTZGRJLN3JASA";
    const PUBLIC_RAW: [u8; X25519_PUBLIC_KEY_SIZE] = [
        0x01u8, 0x2eu8, 0x20u8, 0x88u8, 0x83u8, 0x4fu8, 0x55u8, 0x4du8, 0xf3u8, 0x06u8, 0x33u8,
        0x33u8, 0xf7u8, 0xd4u8, 0xf5u8, 0x19u8, 0x01u8, 0xf1u8, 0xffu8, 0x01u8, 0xfdu8, 0x60u8,
        0x07u8, 0xa6u8, 0x6cu8, 0x9eu8, 0x4du8, 0x14u8, 0xadu8, 0xbbu8, 0x48u8, 0x24u8,
    ];

    // ensure we can convert from raw as expected
    assert!(&X25519PrivateKey::from_raw(&SECRET_RAW).to_base64() == SECRET_BASE64);
    assert!(&X25519PublicKey::from_raw(&PUBLIC_RAW).to_base32() == PUBLIC_BASE32);

    // ensure we can round-trip as expected
    assert!(&X25519PrivateKey::from_base64(&SECRET_BASE64)?.to_base64() == SECRET_BASE64);
    assert!(&X25519PublicKey::from_base32(&PUBLIC_BASE32)?.to_base32() == PUBLIC_BASE32);

    // ensure we generate the expected public key from private key
    let private_key = X25519PrivateKey::from_base64(&SECRET_BASE64)?;
    let public_key = X25519PublicKey::from_private_key(&private_key);
    assert!(public_key.to_base32() == PUBLIC_BASE32);

    let message = b"All around me are familiar faces";

    let (signature, signbit) = private_key.sign_message(message)?;
    assert!(signature.verify_x25519(message, &public_key, signbit));

    Ok(())
}
