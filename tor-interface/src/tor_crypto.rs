// standard
use std::convert::TryInto;
use std::iter;
use std::str;

// extern crates
use crypto::digest::Digest;
use crypto::sha3::Sha3;
use data_encoding::{BASE32, BASE32_NOPAD, BASE64};
use data_encoding_macro::new_encoding;
use rand::distributions::Alphanumeric;
use rand::rngs::OsRng;
use rand::Rng;
use signature::Verifier;
use tor_llcrypto::pk::keymanip::*;
use tor_llcrypto::util::rand_compat::RngCompatExt;
use tor_llcrypto::*;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{0}")]
    ParseError(String),
    #[error("{0}")]
    ConversionError(String),
}

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

// Free functions

// securely generate password using OsRng
pub(crate) fn generate_password(length: usize) -> String {
    let password: String = iter::repeat(())
        .map(|()| OsRng.sample(Alphanumeric))
        .map(char::from)
        .take(length)
        .collect();

    password
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

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct X25519PublicKey {
    public_key: pk::curve25519::PublicKey,
}

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
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

    pub fn from_key_blob(key_blob: &str) -> Result<Ed25519PrivateKey, Error> {
        if key_blob.len() != ED25519_PRIVATE_KEYBLOB_LENGTH {
            return Err(Error::ParseError(format!(
                "expects string of length '{}'; received string with length '{}'",
                ED25519_PRIVATE_KEYBLOB_LENGTH,
                key_blob.len()
            )));
        }

        if !key_blob.starts_with(ED25519_PRIVATE_KEYBLOB_HEADER) {
            return Err(Error::ParseError(format!(
                "expects string that begins with '{}'; received '{}'",
                &ED25519_PRIVATE_KEYBLOB_HEADER, &key_blob
            )));
        }

        let base64_key: &str = &key_blob[ED25519_PRIVATE_KEYBLOB_HEADER.len()..];
        let private_key_data = match BASE64.decode(base64_key.as_bytes()) {
            Ok(private_key_data) => private_key_data,
            Err(_) => {
                return Err(Error::ParseError(format!(
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
                return Err(Error::ParseError(format!(
                    "expects decoded private key length '{}'; actual '{}'",
                    ED25519_PRIVATE_KEY_SIZE, private_key_data_len
                )))
            }
        };

        Ok(Ed25519PrivateKey::from_raw(&private_key_data_raw))
    }

    pub fn from_private_x25519(
        x25519_private: &X25519PrivateKey,
    ) -> Result<(Ed25519PrivateKey, SignBit), Error> {
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
                        return Err(Error::ConversionError(format!(
                            "convert_curve25519_to_ed25519_private() returned invalid signbit: {}",
                            invalid_signbit
                        )))
                    }
                },
            ))
        } else {
            Err(Error::ConversionError(
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
    pub fn from_raw(raw: &[u8; ED25519_PUBLIC_KEY_SIZE]) -> Result<Ed25519PublicKey, Error> {
        Ok(Ed25519PublicKey {
            public_key: match pk::ed25519::PublicKey::from_bytes(raw) {
                Ok(public_key) => public_key,
                Err(_) => {
                    return Err(Error::ConversionError(
                        "failed to create ed25519 public key from bytes".to_string(),
                    ))
                }
            },
        })
    }

    pub fn from_service_id(service_id: &V3OnionServiceId) -> Result<Ed25519PublicKey, Error> {
        // decode base32 encoded service id
        let mut decoded_service_id = [0u8; V3_ONION_SERVICE_ID_RAW_SIZE];
        let decoded_byte_count =
            match ONION_BASE32.decode_mut(service_id.as_bytes(), &mut decoded_service_id) {
                Ok(decoded_byte_count) => decoded_byte_count,
                Err(_) => {
                    return Err(Error::ConversionError(format!(
                        "failed to decode '{}' as V3OnionServiceId",
                        service_id
                    )))
                }
            };
        if decoded_byte_count != V3_ONION_SERVICE_ID_RAW_SIZE {
            return Err(Error::ConversionError(format!(
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
    ) -> Result<Ed25519PublicKey, Error> {
        match convert_curve25519_to_ed25519_public(&public_x25519.public_key, signbit.into()) {
            Some(public_key) => Ok(Ed25519PublicKey { public_key }),
            None => Err(Error::ConversionError(
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

impl std::fmt::Debug for Ed25519PublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.public_key.fmt(f)
    }
}

// Ed25519 Signature

impl Ed25519Signature {
    pub fn from_raw(raw: &[u8; ED25519_SIGNATURE_SIZE]) -> Result<Ed25519Signature, Error> {
        Ok(Ed25519Signature {
            signature: match pk::ed25519::Signature::from_bytes(raw) {
                Ok(signature) => signature,
                Err(_) => {
                    return Err(Error::ConversionError(
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

impl std::fmt::Debug for Ed25519Signature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.signature.fmt(f)
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
    pub fn from_base64(base64: &str) -> Result<X25519PrivateKey, Error> {
        if base64.len() != X25519_PRIVATE_KEYBLOB_BASE64_LENGTH {
            return Err(Error::ParseError(format!(
                "expects string of length '{}'; received string with length '{}'",
                X25519_PRIVATE_KEYBLOB_BASE64_LENGTH,
                base64.len()
            )));
        }

        let private_key_data = match BASE64.decode(base64.as_bytes()) {
            Ok(private_key_data) => private_key_data,
            Err(_) => {
                return Err(Error::ParseError(format!(
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
                return Err(Error::ParseError(format!(
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
    pub fn sign_message(&self, message: &[u8]) -> Result<(Ed25519Signature, SignBit), Error> {
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

    pub fn from_base32(base32: &str) -> Result<X25519PublicKey, Error> {
        if base32.len() != X25519_PUBLIC_KEYBLOB_BASE32_LENGTH {
            return Err(Error::ParseError(format!(
                "expects string of length '{}'; received '{}' with length '{}'",
                X25519_PUBLIC_KEYBLOB_BASE32_LENGTH,
                base32,
                base32.len()
            )));
        }

        let public_key_data = match BASE32_NOPAD.decode(base32.as_bytes()) {
            Ok(public_key_data) => public_key_data,
            Err(_) => {
                return Err(Error::ParseError(format!(
                    "failed to decode '{}' as X25519PublicKey",
                    base32
                )))
            }
        };
        let public_key_data_len = public_key_data.len();
        let public_key_data_raw: [u8; X25519_PUBLIC_KEY_SIZE] = match public_key_data.try_into() {
            Ok(public_key_data) => public_key_data,
            Err(_) => {
                return Err(Error::ParseError(format!(
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

    pub fn as_bytes(&self) -> &[u8; X25519_PUBLIC_KEY_SIZE] {
        self.public_key.as_bytes()
    }
}

impl std::fmt::Debug for X25519PublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_base32())
    }
}

// Onion Service Id

impl V3OnionServiceId {
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

    pub fn from_string(service_id: &str) -> Result<V3OnionServiceId, Error> {
        if !V3OnionServiceId::is_valid(service_id) {
            return Err(Error::ParseError(format!(
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
        let truncated_checksum = Self::calc_truncated_checksum(public_key.as_bytes());
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
                let truncated_checksum = Self::calc_truncated_checksum(&public_key);
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

impl std::fmt::Display for V3OnionServiceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        unsafe { write!(f, "{}", str::from_utf8_unchecked(&self.data)) }
    }
}

impl std::fmt::Debug for V3OnionServiceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        unsafe { write!(f, "{}", str::from_utf8_unchecked(&self.data)) }
    }
}
