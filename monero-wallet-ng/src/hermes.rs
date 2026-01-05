//! Hermes is an extension to the Monero protocol intended exclusively for use by the Eigenwallet Atomic Swap protocol.
//! 
//! It allows two parties (Alice and Bob) to communicate messages through a shared Monero wallet.
//! We use Monero to transmit small amounts of data between the parties as it can act as a reliable communication channel.
//! We only transmit a single message per swap so this should not have a significant impact on the chain.
//! We also commit the data on-chain such that either party can later construct a proof to allow another party that either one of the parties has acted in a certain way.
//! 
//! What is the difference to the Monero `PaymentId`s?
//! 1. The message size is not limited to 32 bytes. It could be up to 1060 bytes large but we limit it to 256 bytes to avoid bloating the chain unnecessarily.
//!    This is the primary reason for this custom extension. We need more to transmit around 162 bytes (> 32 bytes) of data.
//! 2. The message is encrypted using the private view key of the shared Monero wallet.
//!    This means it requires the sender to have knowledge of that key.
//!    PaymentIDs use the shared transaction secret.
//!    We use a different shared secret primarily because it simplifies the implementation.
//! 3. The extension could be modified to use a different shared secret that is negotiated off-chain.
//! 
//! Bob sends a message to Alice:
//! 
//! 1. Sending a transaction to the shared Monero wallet with a specially crafted `tx_extra` field that contains an encrypted message.
//! 0. Bob has
//! - his message `m`
//! - the private view key of the shared Monero wallet `v`
//! - the primary address of the shared Monero wallet `addr`
//! - the shared secret key `k` (by applying Keccak256 to the private view key `k = H(v)`)
//! 1. He concatenates: `HERMES_DATA_MARKER (u8) | `l` (length of m) (u16) | `m` | padding bytes until the limit is reached`.
//!    This yields the blob `b`.
//! 2. He generates a random 12-byte nonce `n` and encrypts `b` using ChaCha20 with key `k` and nonce `n`.
//! 2.1 The final encrypted blob is `e = nonce (12 bytes) || ChaCha20(k, n, b)`.
//! 3. He sets this as the `tx_extra` field of the transaction, adds an output of any amount to the shared wallet.
//! 4. He then signs the transaction and broadcasts it to the network.
//! 
//! Alice receives the message by:
//! 0. She computes the shared secret key by applying Keccak256 to the private view key `k = H(v)`
//! 1. Continuously scans the shared Monero wallet for incoming transactions.
//! 2. For each transaction, she extracts the nonce (first 12 bytes) and ciphertext from the `tx_extra` field.
//! 2.1 She decrypts the ciphertext using ChaCha20 with key `k` and the extracted nonce.
//! 2.2 She checks if the decrypted blob starts with the HERMES_DATA_MARKER.
//! 2.2 She then extracts the length of the message `l` from the decrypted blob. She then reads the next `l` bytes as the message `m`.
//! 2.3 Yields the message `m`.
//! 
//! Some notes:
//! - We could push the marker to the front of the already encrypted blob. This would allow Alice to avoid decrypting unnecessary data but it would make us even more fingerprintable.
//! - As we use monero-oxde arbitrary data tx_extra protocol, we will get an additional data marker in front of the encrypted blob. Preventing this would require patching the monero-oxide library.
//! - We add padding until we reach 256 bytes. This is also done to make us a little bit less fingerprintable by making all messages the same size.
//! - We could use steganography to hide the message in outputs but this bloats even more and is complex to implement

// We use 126 as the marker because it is the highest value not interpretable as a continued VarInt excluding the `ARBITRARY_DATA_MARKER` defined by monero-oxide
// which will likely be used by Serai in the future. As this marker is encrypted anyway, this shouldn't even be an issue but we do it to be safe.

// NOTE: This is very much a draft implementation and subject to change.

use chacha20::{ChaCha20, cipher::{KeyIvInit, StreamCipher}};
use monero_oxide::{ed25519::Scalar, primitives::keccak256};
use zeroize::Zeroizing;

pub const MAX_HERMES_MESSAGE_SIZE: usize = 256;
pub const HERMES_DATA_MARKER: u8 = 126;
pub const NONCE_SIZE: usize = 12;

// 1 bytes for the marker
// 2 bytes for the length of the message
// MAX_HERMES_MESSAGE_SIZE bytes for the message
pub const HERMES_BLOB_LENGTH: usize = 1 + 2 + MAX_HERMES_MESSAGE_SIZE;

// The encrypted blob has an additional nonce at the front
pub const ENCRYPTED_HERMES_BLOB_LENGTH: usize = NONCE_SIZE + HERMES_BLOB_LENGTH;

#[derive(Debug, PartialEq)]
enum HermesError {
    MessageTooLong,
    InvalidBlobLength,
}

// TODO: Add a constraint here regarding the length of the message
/// An unencrypted Hermes message without any metadata
/// 
/// This can either be manually constructed or recovered from an encrypted blob by decrypting it.
/// It has a maximum size of 256 bytes.
#[derive(Debug, PartialEq)]
struct HermesMessage(Vec<u8>);

/// A blob that includes an encrypted Hermes message
/// It has the format: nonce (12 bytes) || encrypted blob (259 bytes)
#[derive(Debug, PartialEq)]
struct EncryptedHermesBlob([u8; ENCRYPTED_HERMES_BLOB_LENGTH]);

/// A blob that includes an unencrypted Hermes message
/// It has the format: marker (1 byte) | length of message (2 bytes) | message (256 bytes)
#[derive(Debug, PartialEq)]
struct HermesBlob([u8; HERMES_BLOB_LENGTH]);

impl HermesMessage {
    pub fn new(message: Vec<u8>) -> Result<Self, HermesError> {
        if message.len() > MAX_HERMES_MESSAGE_SIZE {
            return Err(HermesError::MessageTooLong);
        }

        Ok(Self(message))
    }

    fn blob(&self) -> HermesBlob {
        let l = self.0.len();
        let mut b = Vec::with_capacity(1 + 2 + MAX_HERMES_MESSAGE_SIZE);

        // HERMES_DATA_MARKER | length of message (u16) | message | padding bytes until the limit is reached
        b.push(HERMES_DATA_MARKER);
        b.extend_from_slice(&(l as u16).to_le_bytes());
        b.extend_from_slice(&self.0);
        b.resize(1 + 2 + MAX_HERMES_MESSAGE_SIZE, 0);

        HermesBlob(b.try_into().expect("blob is exactly HERMES_BLOB_LENGTH"))
    }
}

impl EncryptedHermesBlob {
    pub fn new(blob: Vec<u8>) -> Result<Self, HermesError> {
        let blob: [u8; ENCRYPTED_HERMES_BLOB_LENGTH] = blob
            .try_into()
            .map_err(|_| HermesError::InvalidBlobLength)?;

        Ok(Self(blob))
    }

    pub fn decrypt(self, private_view_key: Zeroizing<Scalar>) -> Result<HermesBlob, HermesError> {
        let key = shared_secret(private_view_key);
        
        // Extract nonce from the first 12 bytes
        let nonce: [u8; NONCE_SIZE] = self.0[..NONCE_SIZE]
            .try_into()
            .expect("slice is exactly NONCE_SIZE");
        
        // Extract ciphertext (remaining bytes)
        let mut decrypted: [u8; HERMES_BLOB_LENGTH] = self.0[NONCE_SIZE..]
            .try_into()
            .expect("remaining bytes are exactly HERMES_BLOB_LENGTH");
        
        // Decrypt using ChaCha20
        let mut cipher = ChaCha20::new(&key.into(), &nonce.into());
        cipher.apply_keystream(&mut decrypted);

        Ok(HermesBlob(decrypted))
    }
}

impl HermesBlob {
    pub fn new(blob: Vec<u8>) -> Result<Self, HermesError> {
        let blob: [u8; HERMES_BLOB_LENGTH] = blob
            .try_into()
            .map_err(|_| HermesError::InvalidBlobLength)?;

        Ok(Self(blob))
    }

    fn has_marker(&self) -> bool {
        self.0.first() == Some(&HERMES_DATA_MARKER)
    }

    fn message_length(&self) -> u16 {
        u16::from_le_bytes([self.0[1], self.0[2]])
    }

    pub fn validate(&self) -> bool {
        // The marker must be present
        if !self.has_marker() {
            return false;
        }

        // The specified message length must not be greater than the maximum allowed message size
        if self.message_length() > MAX_HERMES_MESSAGE_SIZE as u16 {
            return false;
        }

        // The entire blob must be exactly HERMES_BLOB_LENGTH long
        if self.0.len() != HERMES_BLOB_LENGTH {
            return false;
        }

        return true;
    }

    pub fn encrypt(self, private_view_key: Zeroizing<Scalar>, nonce: [u8; NONCE_SIZE]) -> EncryptedHermesBlob {
        let key = shared_secret(private_view_key);
        
        // Encrypt the blob using ChaCha20
        let mut encrypted = self.0;
        let mut cipher = ChaCha20::new(&key.into(), &nonce.into());
        cipher.apply_keystream(&mut encrypted);
        
        // Prepend nonce to ciphertext
        let mut result = [0u8; ENCRYPTED_HERMES_BLOB_LENGTH];
        result[..NONCE_SIZE].copy_from_slice(&nonce);
        result[NONCE_SIZE..].copy_from_slice(&encrypted);

        EncryptedHermesBlob(result)
    }
}

fn shared_secret(private_view_key: Zeroizing<Scalar>) -> [u8; 32] {
    keccak256(<[u8; 32]>::from(*private_view_key))
}

#[cfg(test)]
mod tests {
    use super::*;
    use monero_oxide::ed25519::Scalar;

    #[test]
    fn rejects_oversized_message() {
        let oversized_data = vec![0u8; MAX_HERMES_MESSAGE_SIZE + 1];
        let result = HermesMessage::new(oversized_data);

        assert_eq!(result, Err(HermesError::MessageTooLong));
    }

    #[test]
    fn accepts_max_size_message() {
        let max_size_data = vec![0u8; MAX_HERMES_MESSAGE_SIZE];
        let result = HermesMessage::new(max_size_data);

        assert!(result.is_ok());
    }

    #[test]
    fn encrypts_blob_with_nonce_prepended() {
        let private_key = Zeroizing::new(Scalar::hash(b"test_key"));
        let nonce = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];

        let message = HermesMessage::new(vec![42]).unwrap();
        let blob = message.blob();
        let encrypted = blob.encrypt(private_key, nonce);

        // Nonce should be prepended to the encrypted blob
        assert_eq!(&encrypted.0[..NONCE_SIZE], &nonce);
    }

    #[test]
    fn decryption_preserves_original_data() {
        let private_key = Zeroizing::new(Scalar::hash(b"test_key"));
        let nonce = [0u8; NONCE_SIZE];
        let original_data = vec![1, 2, 3, 4, 5];

        let message = HermesMessage::new(original_data.clone()).unwrap();
        let blob = message.blob();
        let encrypted = blob.encrypt(private_key.clone(), nonce);
        let decrypted = encrypted.decrypt(private_key).unwrap();

        // Check the message length is preserved
        assert_eq!(decrypted.message_length(), original_data.len() as u16);
        // Check the marker is present
        assert!(decrypted.has_marker());
        // Check the blob validates
        assert!(decrypted.validate());
    }

    #[test]
    fn wrong_key_produces_invalid_blob() {
        let encrypt_key = Zeroizing::new(Scalar::hash(b"key1"));
        let decrypt_key = Zeroizing::new(Scalar::hash(b"key2"));
        let nonce = [0u8; NONCE_SIZE];

        let message = HermesMessage::new(vec![1, 2, 3]).unwrap();
        let blob = message.blob();
        let encrypted = blob.encrypt(encrypt_key, nonce);
        let decrypted = encrypted.decrypt(decrypt_key).unwrap();

        // Wrong key should produce invalid blob (marker won't match)
        assert!(!decrypted.validate());
    }

    #[test]
    fn rejects_wrong_size_encrypted_blob() {
        let too_small = vec![0u8; ENCRYPTED_HERMES_BLOB_LENGTH - 1];
        let result = EncryptedHermesBlob::new(too_small);

        assert_eq!(result, Err(HermesError::InvalidBlobLength));
    }

    #[test]
    fn rejects_wrong_size_blob() {
        let too_large = vec![0u8; HERMES_BLOB_LENGTH + 1];
        let result = HermesBlob::new(too_large);

        assert_eq!(result, Err(HermesError::InvalidBlobLength));
    }

    #[test]
    fn blob_validation_checks_marker() {
        let mut blob_data = vec![0u8; HERMES_BLOB_LENGTH];
        blob_data[0] = 255; // Wrong marker
        blob_data[1] = 1; // Valid length
        blob_data[2] = 0;

        let blob = HermesBlob::new(blob_data).unwrap();
        assert!(!blob.validate());
    }

    #[test]
    fn blob_validation_checks_message_length() {
        let mut blob_data = vec![0u8; HERMES_BLOB_LENGTH];
        blob_data[0] = HERMES_DATA_MARKER;
        blob_data[1] = 1; // Length = 257 (> MAX_HERMES_MESSAGE_SIZE)
        blob_data[2] = 1;

        let blob = HermesBlob::new(blob_data).unwrap();
        assert!(!blob.validate());
    }
}