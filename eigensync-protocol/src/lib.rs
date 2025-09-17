use automerge::Change;
use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use libp2p::request_response::{ProtocolSupport};
use libp2p::swarm::NetworkBehaviour;
use libp2p::{ping, request_response, StreamProtocol};
use serde::{Deserialize, Serialize};
use std::ops::{Deref, DerefMut};
use std::time::Duration;

const PROTOCOL: &str = "/eigensync/1.0.0";

#[derive(NetworkBehaviour)]
pub struct Behaviour {
    ping: ping::Behaviour,
    sync: SyncBehaviour,
}

pub type SyncBehaviour = request_response::cbor::Behaviour<ServerRequest, Response>;

#[derive(Debug, Clone, Copy, Default)]
pub struct EigensyncProtocol;

impl AsRef<str> for EigensyncProtocol {
    fn as_ref(&self) -> &str {
        PROTOCOL
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct SerializedChange(Vec<u8>);

impl SerializedChange {
    pub fn new(data: Vec<u8>) -> Self {
        SerializedChange(data)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.0.clone()
    }

    pub fn sign_and_encrypt(&self, enc_key: &[u8; 32]) -> anyhow::Result<EncryptedChange> {
        let key32 = key32(enc_key)?;
        let aead = XChaCha20Poly1305::new((&key32).into());
        let pt = self.to_bytes();
        let nonce = nonce_from_plaintext(&key32, &pt);
        let ct = aead.encrypt(
            XNonce::from_slice(&nonce),
            Payload { msg: &pt, aad: b"eigensync.v1" },
        )?;
        let mut out = Vec::with_capacity(24 + ct.len());
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ct);
        Ok(EncryptedChange::new(out))
    }
}

impl From<Change> for SerializedChange {
    fn from(mut change: Change) -> Self {
        SerializedChange(change.bytes().to_vec())
    }
}

impl From<SerializedChange> for Change {
    fn from(change: SerializedChange) -> Self {
        Change::from_bytes(change.0).unwrap()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct EncryptedChange(Vec<u8>);

impl EncryptedChange {
    pub fn new(data: Vec<u8>) -> Self {
        EncryptedChange(data)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.0.clone()
    }

    pub fn decrypt_and_verify(self, enc_key: &[u8; 32]) -> anyhow::Result<SerializedChange> {
        let key32 = key32(enc_key)?;
        let aead = XChaCha20Poly1305::new((&key32).into());
        let buf = self.to_bytes();
        anyhow::ensure!(buf.len() >= 24, "ciphertext too short");
        let (nonce, ct) = buf.split_at(24);
        let pt = aead.decrypt(
            XNonce::from_slice(nonce),
            Payload { msg: ct, aad: b"eigensync.v1" },
        )?;
        Ok(SerializedChange::new(pt))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ServerRequest {
    UploadChangesToServer {
        encrypted_changes: Vec<EncryptedChange>
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Response {
    /// When the server has changes the device hasn't yet
    NewChanges {
        changes: Vec<EncryptedChange>,
    },
    Error {
        reason: String,
    },
}

pub fn server() -> Behaviour {
    Behaviour {
        ping: ping::Behaviour::new(ping::Config::default()),
        sync: SyncBehaviour::new(
            vec![(
                StreamProtocol::new(EigensyncProtocol.as_ref()),
                ProtocolSupport::Inbound,
            )],
            request_response::Config::default()
                .with_request_timeout(Duration::from_secs(30))
                .with_max_concurrent_streams(10),
        ),
    }
}

pub fn client() -> Behaviour {
    Behaviour {
        ping: ping::Behaviour::new(ping::Config::default()),
        sync: SyncBehaviour::new(
            vec![(
                StreamProtocol::new(EigensyncProtocol.as_ref()),
                ProtocolSupport::Outbound,
            )],
            request_response::Config::default()
                .with_request_timeout(Duration::from_secs(30))
                .with_max_concurrent_streams(10),
        ),
    }
}

impl Deref for Behaviour {
    type Target = SyncBehaviour;

    fn deref(&self) -> &Self::Target {
        &self.sync
    }
}

impl DerefMut for Behaviour {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.sync
    }
}

fn key32(key: &[u8]) -> anyhow::Result<[u8; 32]> {
    key.try_into().map_err(|_| anyhow::anyhow!("encryption key must be 32 bytes"))
}

fn nonce_from_plaintext(key32: &[u8; 32], pt: &[u8]) -> [u8; 24] {
    let mut h = blake3::Hasher::new_keyed(key32);
    h.update(b"eigensync.v1.nonce");
    h.update(pt);
    let out = h.finalize();
    let mut nonce = [0u8; 24];
    nonce.copy_from_slice(&out.as_bytes()[..24]);
    nonce
}