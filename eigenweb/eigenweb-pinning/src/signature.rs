use libp2p_identity::{PeerId, PublicKey, SigningError};
use serde::{de::Error as SerdeError, ser::SerializeStruct, Deserialize, Serialize};
use sha2::Digest;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedMessage<M> {
    pub message: M,
    pub key: PublicKey,
    pub signature: Vec<u8>,
}

impl<M> SignedMessage<M>
where
    M: Serialize + Clone,
{
    /// Sign the CBOR encoding of `message` with the provided keypair.
    pub fn new(keypair: &libp2p_identity::Keypair, message: M) -> Result<Self, SigningError> {
        let payload = serde_cbor::to_vec(&message).expect("serialization cannot fail");
        let signature = keypair.sign(&payload)?;

        Ok(Self {
            message,
            key: keypair.public(),
            signature,
        })
    }

    /// Verify the signature against the provided peer id.
    pub fn verify_with_peer(&self, supposed_signer: PeerId) -> bool {
        if self.key.to_peer_id() != supposed_signer {
            return false;
        }

        let payload = match self.message_to_vec() {
            Ok(payload) => payload,
            Err(_) => return false,
        };

        self.key.verify(&payload, &self.signature)
    }

    /// Computes the SHA256 hash of the CBOR encoding of the message
    pub fn content_hash(&self) -> MessageHash {
        let message_vec = self.message_to_vec().unwrap();
        MessageHash(sha2::Sha256::digest(message_vec).into())
    }

    fn message_to_vec(&self) -> Result<Vec<u8>, serde_cbor::error::Error> {
        serde_cbor::to_vec(&self.message)
    }

    pub fn message(&self) -> &M {
        &self.message
    }
}

impl<M> Serialize for SignedMessage<M>
where
    M: Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("SignedMessage", 3)?;
        state.serialize_field("message", &self.message)?;
        state.serialize_field("key", &self.key.encode_protobuf())?;
        state.serialize_field("signature", &self.signature)?;
        state.end()
    }
}

impl<'de, M> Deserialize<'de> for SignedMessage<M>
where
    M: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Helper<T> {
            message: T,
            key: Vec<u8>,
            signature: Vec<u8>,
        }

        let helper = Helper::<M>::deserialize(deserializer)?;
        let key = PublicKey::try_decode_protobuf(&helper.key).map_err(SerdeError::custom)?;

        Ok(Self {
            message: helper.message,
            key,
            signature: helper.signature,
        })
    }
}

impl<M> TryFrom<Vec<u8>> for SignedMessage<M>
where
    M: for<'de> Deserialize<'de>,
{
    type Error = serde_cbor::Error;

    fn try_from(bytes: Vec<u8>) -> Result<Self, Self::Error> {
        serde_cbor::from_slice(&bytes)
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageHash([u8; 32]);

impl MessageHash {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_bytes(self) -> [u8; 32] {
        self.0
    }
}

impl From<[u8; 32]> for MessageHash {
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl From<MessageHash> for [u8; 32] {
    fn from(hash: MessageHash) -> Self {
        hash.0
    }
}

impl std::fmt::Debug for MessageHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Print only first 6 hex characters (first 3 bytes)
        write!(f, "{:02x}{:02x}{:02x}", self.0[0], self.0[1], self.0[2])
    }
}

impl std::fmt::Display for MessageHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Print full hash as hex
        for byte in &self.0 {
            write!(f, "{:02x}", byte)?;
        }
        Ok(())
    }
}