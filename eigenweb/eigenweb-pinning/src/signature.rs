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
    pub fn content_hash(&self) -> [u8; 32] {
        let message_vec = self.message_to_vec().unwrap();
        sha2::Sha256::digest(message_vec).into()
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
