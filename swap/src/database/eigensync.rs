use std::{collections::{HashMap, HashSet}, str::FromStr, sync::Arc, time::{Duration, Instant}};

use autosurgeon::{Hydrate, HydrateError, Reconcile, Reconciler};
use autosurgeon::reconcile::NoKey;
use bdk::bitcoin::hashes::hash160::Hash;
use eigensync::EigensyncHandle;
use libp2p::{Multiaddr, PeerId};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
// serde kept via Cargo features; no direct derives used here
use tokio::sync::RwLock;
use uuid::Uuid;
use rust_decimal::Decimal;
use anyhow::anyhow;
use time::UtcDateTime;

use crate::{database::Swap, monero::{LabeledMoneroAddress, MoneroAddressPool, TransferProof}, protocol::{Database, State}};

#[derive(Serialize, Deserialize, Clone, Eq, Hash, PartialEq, Hydrate, Reconcile, Debug)]
struct KeyWrapper<T>(T, String);

impl<T: Serialize> KeyWrapper<T> {
    fn new(key: T) -> Self {
        let json = serde_json::to_string(&key).unwrap();
        Self(key, json)
    }
}

impl<T> AsRef<str> for KeyWrapper<T> {
    fn as_ref(&self) -> &str {
        &self.1
    }
}

impl<T: DeserializeOwned> From<String> for KeyWrapper<T> {
    fn from(s: String) -> Self {
        Self(serde_json::from_str(&s).unwrap(), s)
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct EigensyncDocument {
    // swap_id, swap
    states: HashMap<InnerStateKey, State>, // swap_states table
    // peer_addresses table
    peer_addresses: HashMap<InnerPeerAddressesKey, ()>, // (peer_id, address)
    // peers table
    peers: HashMap<Uuid, PeerId>, //  (swap_id, peer_id)
    // monero_addresses table
    monero_addresses: HashMap<InnerMoneroAddressKey, MoneroAddressValue>, // (swap_id, address) -> (percentage, label)
    // buffered_transfer_proofs table
    buffered_transfer_proofs: HashMap<Uuid, TransferProof>, // (swap_id, proof)
}

#[derive(Debug, Clone, Eq, Hash, PartialEq, Serialize, Deserialize)]
struct MoneroAddressValue(Decimal, String);

#[derive(Debug, Clone, Eq, Hash, PartialEq, Serialize, Deserialize)]
struct InnerMoneroAddressKey(Uuid, Option<String>);

type MoneroAddressKey = KeyWrapper<InnerMoneroAddressKey>;

#[derive(Debug, Clone, Eq, Hash, PartialEq, Serialize, Deserialize)]
struct InnerStateKey(Uuid, UtcDateTime);

type StateKey = KeyWrapper<InnerStateKey>;

#[derive(Debug, Clone, Eq, Hash, PartialEq, Serialize, Deserialize)]
struct InnerPeerAddressesKey(PeerId, Multiaddr);

type PeerAddressesKey = KeyWrapper<InnerPeerAddressesKey>;

#[derive(Debug, Clone, Reconcile, Hydrate, PartialEq, Default)]
struct EigensyncWire {
    states: HashMap<StateKey, String>,
    // encode (peer_id, addr) -> "peer_id|addr", unit value as bool true
    peer_addresses: HashMap<PeerAddressesKey, bool>,
    // swap_id -> peer_id
    peers: HashMap<String, String>,
    // encode (swap_id, address?) -> "swap_id|address_or_-"; store (Decimal, String) as (String, String)
    monero_addresses: HashMap<MoneroAddressKey, String>,
    // buffered_transfer_proofs table
    buffered_transfer_proofs: HashMap<String, String>, // (swap_id, proof)
}

impl From<&EigensyncDocument> for EigensyncWire {
    fn from(src: &EigensyncDocument) -> Self {
        let peer_addresses = src.peer_addresses.iter().map(|(key, _)|
            (KeyWrapper::new(key.clone()), true))
            .collect();

        let monero_addresses = src.monero_addresses.iter().map(|(key, value)| {
            (KeyWrapper::new(key.clone()), serde_json::to_string(value).unwrap())
        }).collect();

        let states = src.states.iter().map(|(key, state)| {
            let state_json = serde_json::to_string(&Swap::from(state.clone())).unwrap();
            (KeyWrapper::new(key.clone()), state_json)
        }).collect();

        let peers = src.peers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();

        let buffered_transfer_proofs = src.buffered_transfer_proofs.iter().map(|(k, v)| (k.to_string(), serde_json::to_string(&v).unwrap())).collect();

        EigensyncWire {
            states,
            peer_addresses,
            peers,
            monero_addresses,
            buffered_transfer_proofs,
        }
    }
}

impl TryFrom<EigensyncWire> for EigensyncDocument {
    type Error = anyhow::Error;
    fn try_from(w: EigensyncWire) -> anyhow::Result<Self> {
        let peer_addresses = w.peer_addresses.into_iter().map(|(k, _)| {
            let (peer_id, addr) = (k.0.0, k.0.1);
            Ok((InnerPeerAddressesKey(peer_id, addr), ()))
        }).collect::<anyhow::Result<HashMap<InnerPeerAddressesKey, ()>>>()?;

        let monero_addresses = w.monero_addresses.into_iter().map(|(k, v)| {
            let value: MoneroAddressValue = serde_json::from_str(&v)?;
            Ok((k.0, value))
        }).collect::<anyhow::Result<HashMap<InnerMoneroAddressKey, MoneroAddressValue>>>()?;

        let states = w
            .states
            .into_iter()
            .map(|(k, v)| {
                let swap_id = k.0.0;
                let timestamp = k.0.1;
                let swap: Swap = serde_json::from_str(&v)?;
                let state: State = swap.into();
                // convert to utc date time from string like "2025-07-28 15:23:12.0 +00"
                Ok((InnerStateKey(swap_id, timestamp), state))
            })
            .collect::<anyhow::Result<HashMap<_, _>>>()?;

        let peers = w.peers.into_iter().map(|(k, v)| {
            let uuid = Uuid::parse_str(&k)?;
            let peer_id = PeerId::from_str(&v)?;
            Ok((uuid, peer_id))
        }).collect::<anyhow::Result<HashMap<_, _>>>()?;

        let buffered_transfer_proofs = w.buffered_transfer_proofs.into_iter().map(|(k, v)| {
            let uuid = Uuid::parse_str(&k)?;
            let proof: TransferProof = serde_json::from_str(&v)?;
            Ok((uuid, proof))
        }).collect::<anyhow::Result<HashMap<_, _>>>()?;
        
        Ok(EigensyncDocument {
            states,
            peer_addresses,
            peers,
            monero_addresses,
            buffered_transfer_proofs,
        })
    }
}

impl Reconcile for EigensyncDocument {
    type Key<'a> = NoKey;

    fn reconcile<R: Reconciler>(&self, reconciler: R) -> Result<(), R::Error> {
        let wire = EigensyncWire::from(self);
        wire.reconcile(reconciler)
    }
}

impl Hydrate for EigensyncDocument {
    fn hydrate_map<D: autosurgeon::ReadDoc>(
        doc: &D,
        obj: &automerge::ObjId,
    ) -> Result<Self, HydrateError> {
        let wire: EigensyncWire = <EigensyncWire as Hydrate>::hydrate_map(doc, obj)?;
        EigensyncDocument::try_from(wire)
            .map_err(|e| HydrateError::unexpected("EigensyncDocument", e.to_string()))
    }
}

pub struct EigensyncDatabaseAdapter {
    eigensync_handle: Arc<RwLock<EigensyncHandle<EigensyncDocument>>>,
    db: Arc<dyn Database + Send + Sync>,
}

impl EigensyncDatabaseAdapter {
    pub fn new(eigensync_handle: Arc<RwLock<EigensyncHandle<EigensyncDocument>>>, sqlite_database: Arc<dyn Database + Send + Sync>) -> Self {
        Self { eigensync_handle, db: sqlite_database }
    }

    pub async fn run(&mut self) -> anyhow::Result<()> {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            tracing::info!("running eigensync sync");
                        
            if let Err(e) = self.upload_states().await {
                tracing::error!("Error uploading states: {:?}", e);
            }

            if let Err(e) = self.download_states().await {
                tracing::error!("Error downloading states: {:?}", e);
            }
        }
    }

    pub async fn upload_states(&self) -> anyhow::Result<()> {
        // get from db -> write into document
        let mut new_states = HashMap::new();
        let mut new_peers = HashMap::new();
        let mut new_addresses = HashMap::new();
        let mut new_proof = HashMap::new();
        let mut new_peer_addresses = HashMap::new();

        let mut document_lock = self.eigensync_handle.write().await;
        let document_state = document_lock.get_document_state().expect("Eigensync document should be present");
        let swap_states = document_state.states;
        let db_address_pools = self.db.get_monero_address_pools().await?;

        for (swap_id, address_pool) in db_address_pools {
            let mut temp_monero_addresses = HashMap::new();
            for labeled in address_pool.iter() {
                let address_opt_str = labeled.address().map(|a| a.to_string());
                let percentage = labeled.percentage();
                let label = labeled.label().to_string();
                temp_monero_addresses.insert(InnerMoneroAddressKey(swap_id, address_opt_str), MoneroAddressValue(percentage, label));
            }

            new_addresses.extend(temp_monero_addresses);
        }

        for (swap_id, state, timestamp) in self.db.get_all_states().await? {
            if swap_states.contains_key(&InnerStateKey(swap_id, timestamp)) {
                continue;
            }

            let peer_id = self.db.get_peer_id(swap_id).await?;
            let proof = self.db.get_buffered_transfer_proof(swap_id).await?;

            new_states.insert(InnerStateKey(swap_id, timestamp), state);
            new_peers.insert(swap_id, peer_id);
            if let Some(proof) = proof {
                new_proof.insert(swap_id, proof);
            }
        }

        let document_peer_addresses = document_state.peer_addresses;
        for (peer_id, addresses) in self.db.get_all_peer_addresses().await? {
            for address in addresses {
                let key = InnerPeerAddressesKey(peer_id, address);
                if !document_peer_addresses.contains_key(&key) {
                    new_peer_addresses.insert(key, ());
                }
            }
        }

        document_lock.modify(|document| {
            document.peers.extend(new_peers.clone());
            document.states.extend(new_states.clone());
            document.monero_addresses.extend(new_addresses.clone());
            document.buffered_transfer_proofs.extend(new_proof.clone());
            document.peer_addresses.extend(new_peer_addresses.clone());
            Ok(())
        })?;

        Ok(())
    }

    pub async fn download_states(&self) -> anyhow::Result<()> {
        // get from document -> write into db
        let document = self.eigensync_handle.write().await.get_document_state().expect("Eigensync document should be present");

        // States table
        let document_states: HashSet<InnerStateKey> = document.states.keys().cloned().collect();
        let db_states = self.db.get_all_states().await?;
        let mut document_states = document_states.into_iter().collect::<Vec<_>>();
        document_states.sort_by_key(|state_key| state_key.1);

        for state_key in document_states {
            let (swap_id, timestamp) = (state_key.0, state_key.1);

            let document_state: State = document
                .states
                .get(&state_key)
                .ok_or_else(|| anyhow!("State not found for key"))?
                .clone();

            if db_states.iter().any(|(_, _, db_timestamp)| db_timestamp == &timestamp) {
                continue;
            }

            let swap_uuid = swap_id;

            if let Err(e) = self.db.insert_existing_state(swap_uuid, document_state, timestamp).await {
                tracing::error!("Error inserting existing state: {:?}", e);
            }
        }
    
        //peer_addresses table
        let document_peer_addresses: HashSet<InnerPeerAddressesKey> = document.peer_addresses.keys().cloned().collect();
        let db_peer_addresses = self.db.get_all_peer_addresses().await?;
        for peer_address_key in document_peer_addresses {
            let (peer_id, address) = (peer_address_key.0, peer_address_key.1);

            if db_peer_addresses.iter().any(|(p, a)| p == &peer_id && a.contains(&address)) {
                continue;
            }

            self.db.insert_address(peer_id, address).await?;
        }

        //peers table
        let document_peers: HashSet<Uuid> = document.peers.keys().cloned().collect();
        for swap_id in document_peers {
            let document_peer = document
                .peers
                .get(&swap_id)
                .ok_or_else(|| anyhow!("Peer not found for key"))?
                .clone();

            if let Ok(peer_id) = self.db.get_peer_id(swap_id).await {
                if peer_id == document_peer {
                    continue;
                }
            }

            self.db.insert_peer_id(swap_id, document_peer).await?;
        }

        //monero_addresses table
        let document_monero_addresses: HashSet<InnerMoneroAddressKey> = document.monero_addresses.keys().cloned().collect();
        let db_monero_addresses = self.db.get_monero_address_pools().await?;
        for monero_address_key in document_monero_addresses {
            let (swap_id, address) = (monero_address_key.0.clone(), monero_address_key.1.clone());

            // We need to check the combination of swap_id and address
            if db_monero_addresses.iter().any(|(s, pool)| {
                s == &swap_id && pool.addresses().iter().any(|addr_opt| {
                    match (&address, addr_opt) {
                        (Some(addr_str), Some(addr)) => addr_str == &addr.to_string(),
                        (None, None) => true, // Both are internal addresses
                        _ => false,
                    }
                })
            }) {
                continue;
            }

            // Get the percentage and label from the document
            let MoneroAddressValue(percentage, label) = document.monero_addresses.get(&monero_address_key)
                .ok_or_else(|| anyhow!("Monero address data not found"))?;
            
            // Create a MoneroAddressPool with the address data
            let labeled = match &address {
                Some(addr_str) => {
                    let addr = monero::Address::from_str(addr_str)?;
                    LabeledMoneroAddress::with_address(addr, *percentage, label.clone())?
                }
                None => {
                    LabeledMoneroAddress::with_internal_address(*percentage, label.clone())?
                }
            };

            let address_pool = MoneroAddressPool::new(vec![labeled]);

            self.db.insert_monero_address_pool(swap_id, address_pool).await?;
        }

        //buffered_transfer_proofs table
        let document_buffered_transfer_proofs: HashSet<Uuid> = document.buffered_transfer_proofs.keys().cloned().collect();
        for swap_id in document_buffered_transfer_proofs {
            let db_buffered_transfer_proof = self.db.get_buffered_transfer_proof(swap_id).await?;
            let document_buffered_transfer_proof = document.buffered_transfer_proofs.get(&swap_id)
                .ok_or_else(|| anyhow!("Buffered transfer proof not found for key"))?
                .clone();            
            
            if db_buffered_transfer_proof == Some(document_buffered_transfer_proof.clone()) {
                continue;
            }
            
            self.db.insert_buffered_transfer_proof(swap_id, document_buffered_transfer_proof).await?;
        }

        Ok(())
    }
}