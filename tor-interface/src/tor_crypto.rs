// standard
use std::convert::TryInto;
use std::str;

// extern crates
use curve25519_dalek::Scalar;
use data_encoding::{BASE32_NOPAD, BASE64};
use data_encoding_macro::new_encoding;
#[cfg(feature = "legacy-tor-provider")]
use rand::distributions::Alphanumeric;
use rand::rngs::OsRng;
#[cfg(feature = "legacy-tor-provider")]
use rand::Rng;
use sha3::{Digest, Sha3_256};
use static_assertions::const_assert_eq;
use tor_llcrypto::pk::keymanip::*;
use tor_llcrypto::*;

/// Represents various errors that can occur in the tor_crypto module.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// A error encountered converting a String to a tor_crypto type
    #[error("{0}")]
    ParseError(String),

    /// An error encountered converting between tor_crypto types
    #[error("{0}")]
    ConversionError(String),

    /// An error encountered converting from a raw byte representation
    #[error("invalid key")]
    KeyInvalid,
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
pub const V3_ONION_SERVICE_ID_STRING_LENGTH: usize = 56;
/// The number of bytes needed to store onion service id as an ASCII c-string (including null-terminator)
pub const V3_ONION_SERVICE_ID_STRING_SIZE: usize = 57;
const_assert_eq!(
    V3_ONION_SERVICE_ID_STRING_SIZE,
    V3_ONION_SERVICE_ID_STRING_LENGTH + 1
);
/// The number of bytes needed to store base64 encoded ed25519 private key as an ASCII c-string (not including null-terminator)
pub const ED25519_PRIVATE_KEYBLOB_BASE64_LENGTH: usize = 88;
/// key klob header string
const ED25519_PRIVATE_KEY_KEYBLOB_HEADER: &str = "ED25519-V3:";
/// The number of bytes needed to store the keyblob header
pub const ED25519_PRIVATE_KEY_KEYBLOB_HEADER_LENGTH: usize = 11;
const_assert_eq!(
    ED25519_PRIVATE_KEY_KEYBLOB_HEADER_LENGTH,
    ED25519_PRIVATE_KEY_KEYBLOB_HEADER.len()
);
/// The number of bytes needed to store ed25519 private keyblob as an ASCII c-string (not including a null terminator)
pub const ED25519_PRIVATE_KEY_KEYBLOB_LENGTH: usize = 99;
const_assert_eq!(
    ED25519_PRIVATE_KEY_KEYBLOB_LENGTH,
    ED25519_PRIVATE_KEY_KEYBLOB_HEADER_LENGTH + ED25519_PRIVATE_KEYBLOB_BASE64_LENGTH
);
/// The number of bytes needed to store ed25519 private keyblob as an ASCII c-string (including a null terminator)
pub const ED25519_PRIVATE_KEY_KEYBLOB_SIZE: usize = 100;
const_assert_eq!(
    ED25519_PRIVATE_KEY_KEYBLOB_SIZE,
    ED25519_PRIVATE_KEY_KEYBLOB_LENGTH + 1
);
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
pub const X25519_PRIVATE_KEY_BASE64_LENGTH: usize = 44;
/// The number of bytes needed to store base64 encoded x25519 private key as an ASCII c-string (including a null terminator)
pub const X25519_PRIVATE_KEY_BASE64_SIZE: usize = 45;
const_assert_eq!(
    X25519_PRIVATE_KEY_BASE64_SIZE,
    X25519_PRIVATE_KEY_BASE64_LENGTH + 1
);
/// The number of bytes needed to store base32 encoded x25519 public key as an ASCII c-string (not including null-terminator)
pub const X25519_PUBLIC_KEY_BASE32_LENGTH: usize = 52;
/// The number of bytes needed to store base32 encoded x25519 public key as an ASCII c-string (including a null terminator)
pub const X25519_PUBLIC_KEY_BASE32_SIZE: usize = 53;
const_assert_eq!(
    X25519_PUBLIC_KEY_BASE32_SIZE,
    X25519_PUBLIC_KEY_BASE32_LENGTH + 1
);

const ONION_BASE32: data_encoding::Encoding = new_encoding! {
    symbols: "abcdefghijklmnopqrstuvwxyz234567",
    padding: '=',
};

// Free functions

// securely generate password using OsRng
#[cfg(feature = "legacy-tor-provider")]
pub(crate) fn generate_password(length: usize) -> String {
    let password: String = std::iter::repeat(())
        .map(|()| OsRng.sample(Alphanumeric))
        .map(char::from)
        .take(length)
        .collect();

    password
}

// Struct deinitions

/// An ed25519 private key.
///
/// This key type is used with [`crate::tor_provider::TorProvider`] trait for hosting onion-services and can be convertd to an [`Ed25519PublicKey`]. It can also be used to sign messages and create an [`Ed25519Signature`].
pub struct Ed25519PrivateKey {
    expanded_keypair: pk::ed25519::ExpandedKeypair,
}

/// An ed25519 public key.
///
/// This key type is derived from [`Ed25519PrivateKey`] and can be converted to a [`V3OnionServiceId`]. It can also be used to verify a [`Ed25519Signature`].
#[derive(Clone)]
pub struct Ed25519PublicKey {
    public_key: pk::ed25519::PublicKey,
}

/// An ed25519 cryptographic signature
#[derive(Clone)]
pub struct Ed25519Signature {
    signature: pk::ed25519::Signature,
}

/// An x25519 private key
#[derive(Clone)]
pub struct X25519PrivateKey {
    secret_key: pk::curve25519::StaticSecret,
}

/// An x25519 public key
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct X25519PublicKey {
    public_key: pk::curve25519::PublicKey,
}

/// A v3 onion-service id
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct V3OnionServiceId {
    data: [u8; V3_ONION_SERVICE_ID_STRING_LENGTH],
}

/// An enum representing a single bit
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

// which validation method to use when constructing an ed25519 expanded key from
// a byte array
enum FromRawValidationMethod {
    // expanded ed25519 keys coming from legacy c-tor daemon; the scalar portion
    // is clamped, but not reduced
    #[cfg(feature = "legacy-tor-provider")]
    LegacyCTor,
    // expanded ed25519 keys coming from ed25519-dalek crate; the scalar portion
    // has been clamped AND reduced
    Ed25519Dalek,
}

/// A wrapper around `tor_llcrypto::pk::ed25519::ExpandedKeypair`.
impl Ed25519PrivateKey {
    /// Securely generate a new `Ed25519PrivateKey`.
    pub fn generate() -> Ed25519PrivateKey {
        let csprng = &mut OsRng;
        let keypair = pk::ed25519::Keypair::generate(csprng);

        Ed25519PrivateKey {
            expanded_keypair: pk::ed25519::ExpandedKeypair::from(&keypair),
        }
    }

    fn from_raw_impl(
        raw: &[u8; ED25519_PRIVATE_KEY_SIZE],
        method: FromRawValidationMethod,
    ) -> Result<Ed25519PrivateKey, Error> {
        // see: https://gitlab.torproject.org/tpo/core/arti/-/issues/1343
        match method {
            #[cfg(feature = "legacy-tor-provider")]
            FromRawValidationMethod::LegacyCTor => {
                // Verify the scalar portion of the expanded key has been clamped
                // see: https://gitlab.torproject.org/tpo/core/arti/-/issues/1021
                if !(raw[0] == raw[0] & 248 && raw[31] == (raw[31] & 63) | 64) {
                    return Err(Error::KeyInvalid);
                }
            }
            FromRawValidationMethod::Ed25519Dalek => {
                // Verify the scalar is non-zero and it has been reduced
                let scalar: [u8; 32] = raw[..32].try_into().unwrap();
                if scalar.iter().all(|&x| x == 0x00u8) {
                    return Err(Error::KeyInvalid);
                }
                let reduced_scalar = Scalar::from_bytes_mod_order(scalar).to_bytes();
                if scalar != reduced_scalar {
                    return Err(Error::KeyInvalid);
                }
            }
        }

        if let Some(expanded_keypair) = pk::ed25519::ExpandedKeypair::from_secret_key_bytes(*raw) {
            Ok(Ed25519PrivateKey { expanded_keypair })
        } else {
            Err(Error::KeyInvalid)
        }
    }

    /// Attempt to create an `Ed25519PrivateKey` from an array of bytes. Not all byte buffers of the required size can create a valid `Ed25519PrivateKey`. Only buffers derived from [`Ed25519PrivateKey::to_bytes()`] are required to convert correctly.
    ///
    /// To securely generate a valid `Ed25519PrivateKey`, use [`Ed25519PrivateKey::generate()`].
    pub fn from_raw(raw: &[u8; ED25519_PRIVATE_KEY_SIZE]) -> Result<Ed25519PrivateKey, Error> {
        Self::from_raw_impl(raw, FromRawValidationMethod::Ed25519Dalek)
    }

    fn from_key_blob_impl(
        key_blob: &str,
        method: FromRawValidationMethod,
    ) -> Result<Ed25519PrivateKey, Error> {
        if key_blob.len() != ED25519_PRIVATE_KEY_KEYBLOB_LENGTH {
            return Err(Error::ParseError(format!(
                "expects string of length '{}'; received string with length '{}'",
                ED25519_PRIVATE_KEY_KEYBLOB_LENGTH,
                key_blob.len()
            )));
        }

        if !key_blob.starts_with(ED25519_PRIVATE_KEY_KEYBLOB_HEADER) {
            return Err(Error::ParseError(format!(
                "expects string that begins with '{}'; received '{}'",
                &ED25519_PRIVATE_KEY_KEYBLOB_HEADER, &key_blob
            )));
        }

        let base64_key: &str = &key_blob[ED25519_PRIVATE_KEY_KEYBLOB_HEADER.len()..];
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

        Ed25519PrivateKey::from_raw_impl(&private_key_data_raw, method)
    }

    #[cfg(feature = "legacy-tor-provider")]
    pub(crate) fn from_key_blob_legacy(key_blob: &str) -> Result<Ed25519PrivateKey, Error> {
        Self::from_key_blob_impl(key_blob, FromRawValidationMethod::LegacyCTor)
    }

    /// Create an `Ed25519PrivateKey` from a [`String`] in the legacy c-tor daemon key blob format used in the `ADD_ONION` control-port command. From the c-tor control [specification](https://spec.torproject.org/control-spec/commands.html#add_onion):
    /// > For a "ED25519-V3" key is the Base64 encoding of the concatenation of the 32-byte ed25519 secret scalar in little-endian and the 32-byte ed25519 PRF secret.
    ///
    /// Only key blob strings derived from [`Ed25519PrivateKey::to_key_blob()`] are required to convert correctly.
    pub fn from_key_blob(key_blob: &str) -> Result<Ed25519PrivateKey, Error> {
        Self::from_key_blob_impl(key_blob, FromRawValidationMethod::Ed25519Dalek)
    }

    /// Construct an `Ed25519PrivateKEy` from an [`X25519PrivateKey`].
    pub fn from_private_x25519(
        x25519_private: &X25519PrivateKey,
    ) -> Result<(Ed25519PrivateKey, SignBit), Error> {
        if let Some((result, signbit)) =
            convert_curve25519_to_ed25519_private(&x25519_private.secret_key)
        {
            Ok((
                Ed25519PrivateKey {
                    expanded_keypair: result,
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

    /// Write `Ed25519PrivateKey` to a c-tor key blob formatted [`String`].
    pub fn to_key_blob(&self) -> String {
        let mut key_blob = ED25519_PRIVATE_KEY_KEYBLOB_HEADER.to_string();
        key_blob.push_str(&BASE64.encode(&self.expanded_keypair.to_secret_key_bytes()));

        key_blob
    }

    /// Sign the provided message and return an [`Ed25519Signature`].
    /// ## ⚠ Warning ⚠
    ///Only ever sign messages the private key owner controls the contents of!
    pub fn sign_message(&self, message: &[u8]) -> Ed25519Signature {
        let signature = self.expanded_keypair.sign(message);
        Ed25519Signature { signature }
    }

    /// Convert this private key to an array of bytes.
    pub fn to_bytes(&self) -> [u8; ED25519_PRIVATE_KEY_SIZE] {
        self.expanded_keypair.to_secret_key_bytes()
    }

    #[cfg(feature = "arti-client-tor-provider")]
    pub(crate) fn inner(&self) -> &pk::ed25519::ExpandedKeypair {
        &self.expanded_keypair
    }
}

impl PartialEq for Ed25519PrivateKey {
    fn eq(&self, other: &Self) -> bool {
        self.to_bytes().eq(&other.to_bytes())
    }
}

impl Clone for Ed25519PrivateKey {
    fn clone(&self) -> Ed25519PrivateKey {
        match Ed25519PrivateKey::from_raw(&self.to_bytes()) {
            Ok(ed25519_private_key) => ed25519_private_key,
            Err(_) => unreachable!(),
        }
    }
}

impl std::fmt::Debug for Ed25519PrivateKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "--- ed25519 private key ---")
    }
}

/// A wrapper around `tor_llcrypto::pk::ed25519::PublicKey`
impl Ed25519PublicKey {
    /// Construct an `Ed25519PublicKey` from an array of bytes. Not all byte buffers of the required size can create a valid `Ed25519PublicKey`. Only buffers derived from [`Ed25519PublicKey::as_bytes()`] are required to convert correctly.
    pub fn from_raw(raw: &[u8; ED25519_PUBLIC_KEY_SIZE]) -> Result<Ed25519PublicKey, Error> {
        Ok(Ed25519PublicKey {
            public_key: match pk::ed25519::PublicKey::from_bytes(raw) {
                Ok(public_key) => public_key,
                Err(_) => return Err(Error::KeyInvalid),
            },
        })
    }

    /// Construct an `Ed25519PublicKey` from a [`V3OnionServiceId`].
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

    /// Construct an `Ed25519PublicKey` from an [`Ed25519PrivateKey`].
    pub fn from_private_key(private_key: &Ed25519PrivateKey) -> Ed25519PublicKey {
        Ed25519PublicKey {
            public_key: *private_key.expanded_keypair.public(),
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

    /// View this public key as an array of bytes
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


/// A wrapper around `tor_llcrypto::pk::ed25519::Signature`
impl Ed25519Signature {
    /// Construct an `Ed25519Signature` from an array of bytes.
    pub fn from_raw(raw: &[u8; ED25519_SIGNATURE_SIZE]) -> Result<Ed25519Signature, Error> {
        // todo: message cannot fail so should not return a Result<>
        Ok(Ed25519Signature {
            signature: pk::ed25519::Signature::from_bytes(raw),
        })
    }

    /// Verify this `Ed25519Signature` for the given message and [`Ed25519PublicKey`].
    pub fn verify(&self, message: &[u8], public_key: &Ed25519PublicKey) -> bool {
        if let Ok(()) = public_key
            .public_key
            .verify_strict(message, &self.signature)
        {
            return true;
        }
        false
    }

    /// Verify this `Ed25519Signature` for the given message, [`X25519PublicKey`], and [`SignBit`]. This signature must have been created by first converting an [`X25519PrivateKey`] to a [`Ed25519PrivateKey`] and [`SignBit`], and then signing the message using this [`Ed25519PrivateKey`]. This method verifies the signature using the [`Ed25519PublicKey`] derived from the provided  [`X25519PublicKey`] and [`SignBit`].
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

    /// Convert this signature to an array of bytes
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

/// A wrapper around `tor_llcrypto::pk::curve25519::StaticSecret`
impl X25519PrivateKey {
    /// Securely generate a new `X25519PrivateKey`
    pub fn generate() -> X25519PrivateKey {
        let csprng = &mut OsRng;
        X25519PrivateKey {
            secret_key: pk::curve25519::StaticSecret::random_from_rng(csprng),
        }
    }

    /// Attempt to create an `X25519PrivateKey` from an array of bytes. Not all byte buffers of the required size can create a valid `X25519PrivateKey`. Only buffers derived from [`X25519PrivateKey::to_bytes()`] are required to convert correctly.
    ///
    /// To securely generate a valid `X25519PrivateKey`, use [`X25519PrivateKey::generate()`].
    pub fn from_raw(raw: &[u8; X25519_PRIVATE_KEY_SIZE]) -> Result<X25519PrivateKey, Error> {
        // see: https://docs.rs/x25519-dalek/2.0.0-pre.1/src/x25519_dalek/x25519.rs.html#197
        if raw[0] == raw[0] & 240 && raw[31] == (raw[31] & 127) | 64 {
            Ok(X25519PrivateKey {
                secret_key: pk::curve25519::StaticSecret::from(*raw),
            })
        } else {
            Err(Error::KeyInvalid)
        }
    }

    /// Create an `X25519PrivateKey` from a [`String`] in the legacy c-tor daemon key blob format used in the `ONION_CLIENT_AUTH_ADD` control-port command. From the c-tor control [specification](https://spec.torproject.org/control-spec/commands.html#onion_client_auth_add):
    /// > ```text
    /// > PrivateKeyBlob = base64 encoding of x25519 key
    /// > ```
    ///
    /// Only key blob strings derived from [`X25519PrivateKey::to_base64()`] are required to convert correctly.
    pub fn from_base64(base64: &str) -> Result<X25519PrivateKey, Error> {
        // todo: see if this should be from/to key blob like with ed25519 rather than base64
        if base64.len() != X25519_PRIVATE_KEY_BASE64_LENGTH {
            return Err(Error::ParseError(format!(
                "expects string of length '{}'; received string with length '{}'",
                X25519_PRIVATE_KEY_BASE64_LENGTH,
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

        X25519PrivateKey::from_raw(&private_key_data_raw)
    }

    /// Sign the provided message and return an [`Ed25519Signature`] and [`SignBit`].
    ///
    /// This method first converts this `X25519PrivateKey` to an [`Ed25519PrivateKey`] and [`SignBit`]. Then, the message is signed using the derived [`Ed25519PrivateKey`]. To verify the signature, both the [`X25519PublicKey`] and this calculated [`SignBit`] are required.
    ///
    /// ## ⚠ Warning ⚠
    ///Only ever sign messages the private key owner controls the contents of!
    pub fn sign_message(&self, message: &[u8]) -> Result<(Ed25519Signature, SignBit), Error> {
        let (ed25519_private, signbit) = Ed25519PrivateKey::from_private_x25519(self)?;
        Ok((ed25519_private.sign_message(message), signbit))
    }

    /// Write `X25519PrivateKey` to a base64 encocded [`String`].
    pub fn to_base64(&self) -> String {
        BASE64.encode(&self.secret_key.to_bytes())
    }

    /// Convert this private key to an array of bytes.
    pub fn to_bytes(&self) -> [u8; X25519_PRIVATE_KEY_SIZE] {
        self.secret_key.to_bytes()
    }

    #[cfg(feature = "arti-client-tor-provider")]
    pub(crate) fn inner(&self) -> &pk::curve25519::StaticSecret {
        &self.secret_key
    }
}

impl PartialEq for X25519PrivateKey {
    fn eq(&self, other: &Self) -> bool {
        self.secret_key.to_bytes() == other.secret_key.to_bytes()
    }
}

impl std::fmt::Debug for X25519PrivateKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "--- x25519 private key ---")
    }
}

/// A wrapper around `tor_llcrypto::pk::curve25519::PublicKey`
impl X25519PublicKey {
    /// Construct an `X25519PublicKey` from an [`X25519PrivateKey`].
    pub fn from_private_key(private_key: &X25519PrivateKey) -> X25519PublicKey {
        X25519PublicKey {
            public_key: pk::curve25519::PublicKey::from(&private_key.secret_key),
        }
    }

    /// Construct an `X25519PublicKey` from an array of bytes.
    pub fn from_raw(raw: &[u8; X25519_PUBLIC_KEY_SIZE]) -> X25519PublicKey {
        X25519PublicKey {
            public_key: pk::curve25519::PublicKey::from(*raw),
        }
    }

    /// Create an `X25519PublicKey` from a [`String`] in the legacy c-tor daemon key base32 format used in the `ADD_ONION` control-port command. From the c-tor control [specification](https://spec.torproject.org/control-spec/commands.html#add_onion):
    /// > ```text
    /// > V3Key = The client's base32-encoded x25519 public key, using only the key
    /// >         part of rend-spec-v3.txt section G.1.2 (v3 only).
    /// > ```
    ///
    /// Only key base32 strings derived from [`X25519PublicKey::to_base32()`] are required to convert correctly.
    pub fn from_base32(base32: &str) -> Result<X25519PublicKey, Error> {
        if base32.len() != X25519_PUBLIC_KEY_BASE32_LENGTH {
            return Err(Error::ParseError(format!(
                "expects string of length '{}'; received '{}' with length '{}'",
                X25519_PUBLIC_KEY_BASE32_LENGTH,
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

    /// Write `X25519PublicKey` to a base32 encocded [`String`].
    pub fn to_base32(&self) -> String {
        BASE32_NOPAD.encode(self.public_key.as_bytes())
    }

    /// View this public key as an array of bytes
    pub fn as_bytes(&self) -> &[u8; X25519_PUBLIC_KEY_SIZE] {
        self.public_key.as_bytes()
    }

    #[cfg(feature = "arti-client-tor-provider")]
    pub(crate) fn inner(&self) -> &pk::curve25519::PublicKey {
        &self.public_key
    }

}

impl std::fmt::Debug for X25519PublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_base32())
    }
}

/// Strongly-typed representation of a v3 onion-service id
impl V3OnionServiceId {
    // see https://github.com/torproject/torspec/blob/main/rend-spec-v3.txt#L2143
    fn calc_truncated_checksum(
        public_key: &[u8; ED25519_PUBLIC_KEY_SIZE],
    ) -> [u8; TRUNCATED_CHECKSUM_SIZE] {
        let mut hasher = Sha3_256::new();

        // calculate checksum
        hasher.update(b".onion checksum");
        hasher.update(public_key);
        hasher.update([0x03u8]);
        let hash_bytes = hasher.finalize();

        [hash_bytes[0], hash_bytes[1]]
    }

    /// Create a `V3OnionServiceId` from a [`String`] in the version 3 onion service digest format. From the tor address [specification](https://spec.torproject.org/address-spec.html#onion):
    /// > ```text
    /// > onion_address = base32(PUBKEY | CHECKSUM | VERSION)
    /// > CHECKSUM = H(".onion checksum" | PUBKEY | VERSION)[:2]
    /// >
    /// > where:
    /// > - PUBKEY is the 32 bytes ed25519 master pubkey of the onion service.
    /// > - VERSION is a one byte version field (default value '\x03')
    /// > - ".onion checksum" is a constant string
    /// > - H is SHA3-256
    /// > - CHECKSUM is truncated to two bytes before inserting it in onion_address
    /// > ```
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

    /// Create a `V3OnionServiceId` from an [`Ed25519PublicKey`].
    pub fn from_public_key(public_key: &Ed25519PublicKey) -> V3OnionServiceId {
        let mut raw_service_id = [0u8; V3_ONION_SERVICE_ID_RAW_SIZE];

        raw_service_id[..ED25519_PUBLIC_KEY_SIZE].copy_from_slice(&public_key.as_bytes()[..]);
        let truncated_checksum = Self::calc_truncated_checksum(public_key.as_bytes());
        raw_service_id[V3_ONION_SERVICE_ID_CHECKSUM_OFFSET] = truncated_checksum[0];
        raw_service_id[V3_ONION_SERVICE_ID_CHECKSUM_OFFSET + 1] = truncated_checksum[1];
        raw_service_id[V3_ONION_SERVICE_ID_VERSION_OFFSET] = 0x03u8;

        let mut service_id = [0u8; V3_ONION_SERVICE_ID_STRING_LENGTH];
        // panics on wrong buffer size, but given our constant buffer sizes should be fine
        ONION_BASE32.encode_mut(&raw_service_id, &mut service_id);

        V3OnionServiceId { data: service_id }
    }

    /// Create a `V3OnionServiceId` from an [`Ed25519PrivateKey`].
    pub fn from_private_key(private_key: &Ed25519PrivateKey) -> V3OnionServiceId {
        Self::from_public_key(&Ed25519PublicKey::from_private_key(private_key))
    }

    /// Determine if the provided string is a valid representation of a `V3OnionServiceId`
    pub fn is_valid(service_id: &str) -> bool {
        if service_id.len() != V3_ONION_SERVICE_ID_STRING_LENGTH {
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

    /// View this service id as an array of bytes
    pub fn as_bytes(&self) -> &[u8; V3_ONION_SERVICE_ID_STRING_LENGTH] {
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
