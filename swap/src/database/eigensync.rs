use std::{collections::{HashMap, HashSet}, str::FromStr, sync::Arc, time::Duration};

use autosurgeon::{Hydrate, HydrateError, Reconcile, Reconciler};
use autosurgeon::reconcile::NoKey;
use eigensync::EigensyncHandle;
use libp2p::{Multiaddr, PeerId};
// serde kept via Cargo features; no direct derives used here
use tokio::sync::RwLock;
use uuid::Uuid;
use rust_decimal::Decimal;
use anyhow::anyhow;

use crate::{database::Swap, monero::{LabeledMoneroAddress, MoneroAddressPool, TransferProof}, protocol::{Database, State}};


#[derive(Debug, Clone, PartialEq, Default)]
pub struct EigensyncDocument {
    // swap_id, swap
    states: HashMap<StateKey, State>, // swap_states table
    // peer_addresses table
    peer_addresses: HashMap<PeerAddressesKey, ()>, // (peer_id, address)
    // peers table
    peers: HashMap<Uuid, PeerId>, //  (swap_id, peer_id)
    // monero_addresses table
    monero_addresses: HashMap<MoneroAddressKey, (Decimal, String)>, // (swap_id, address) -> (percentage, label)
    // buffered_transfer_proofs table
    buffered_transfer_proofs: HashMap<Uuid, TransferProof>, // (swap_id, proof)
}

#[derive(Debug, Clone, Eq, Hash, PartialEq)]
struct MoneroAddressKey((Uuid, Option<String>));

#[derive(Debug, Clone, Eq, Hash, PartialEq)]
struct StateKey((Uuid, i64));

#[derive(Debug, Clone, Eq, Hash, PartialEq)]
struct PeerAddressesKey((PeerId, Multiaddr));

#[derive(Debug, Clone, Reconcile, Hydrate, PartialEq, Default)]
struct EigensyncWire {
    states: HashMap<String, String>,
    // encode (peer_id, addr) -> "peer_id|addr", unit value as bool true
    peer_addresses: HashMap<String, bool>,
    // swap_id -> peer_id
    peers: HashMap<String, String>,
    // encode (swap_id, address?) -> "swap_id|address_or_-"; store (Decimal, String) as (String, String)
    monero_addresses: HashMap<String, String>,
    // buffered_transfer_proofs table
    buffered_transfer_proofs: HashMap<String, String>, // (swap_id, proof)
}

impl From<&EigensyncDocument> for EigensyncWire {
    fn from(src: &EigensyncDocument) -> Self {
        fn enc_pair(a: &str, b: &str) -> String { format!("{a}|{b}") }
        fn enc_mo_key(swap: &Uuid, addr: &Option<String>) -> String {
            enc_pair(swap.to_string().as_str(), addr.as_deref().unwrap_or("-"))
        }

        let peer_addresses = src.peer_addresses.iter()
            .map(|(key, _)| (enc_pair(&key.0.0.to_string(), &key.0.1.to_string()), true))
            .collect();

        let monero_addresses = src.monero_addresses.iter().map(|(key, (pct, label))| {
            (enc_mo_key(&key.0.0, &key.0.1), format!("{pct}|{label}"))
        }).collect();

        let states = src.states.iter().map(|(key, state)| {
            let key_str = format!("{}_{}", key.0.0, key.0.1);
            let state_json = serde_json::to_string(&Swap::from(state.clone())).unwrap();
            (key_str, state_json)
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
        fn dec_pair(s: &str) -> anyhow::Result<(String, String)> {
            let mut it = s.splitn(2, '|');
            let a = it.next().ok_or_else(|| anyhow::anyhow!("bad key"))?.to_string();
            let b = it.next().ok_or_else(|| anyhow::anyhow!("bad key"))?.to_string();
            Ok((a, b))
        }
        fn dec_mo_key(s: &str) -> anyhow::Result<(Uuid, Option<String>)> {
            let (swap, addr) = dec_pair(s)?;
            let swap_id = Uuid::parse_str(&swap)?;
            Ok((swap_id, if addr == "-" { None } else { Some(addr) }))
        }

        let peer_addresses = w.peer_addresses.into_iter().map(|(k, _)| {
            let (p, a) = dec_pair(&k)?;
            let peer_id = PeerId::from_str(&p)?;
            let addr = Multiaddr::from_str(&a)?;
            Ok((PeerAddressesKey((peer_id, addr)), ()))
        }).collect::<anyhow::Result<HashMap<PeerAddressesKey, ()>>>()?;

        let monero_addresses = w.monero_addresses.into_iter().map(|(k, v)| {
            let (swap, addr) = dec_mo_key(&k)?;
            let parts: Vec<&str> = v.split('|').collect();
            let pct = parts[0];
            let label = parts[1];
            let dec = Decimal::from_str(pct)?;
            Ok((MoneroAddressKey((swap, addr)), (dec, label.to_string())))
        }).collect::<anyhow::Result<HashMap<MoneroAddressKey, (Decimal, String)>>>()?;

        let states = w
            .states
            .into_iter()
            .map(|(k, v)| {
                let mut it = k.splitn(2, '_');
                let swap_id_s = it.next().ok_or_else(|| anyhow!("bad state key"))?;
                let ts_s = it.next().ok_or_else(|| anyhow!("bad state key"))?;
                let swap_id = Uuid::parse_str(swap_id_s)?;
                let timestamp = ts_s.parse::<i64>()?;
                let swap: Swap = serde_json::from_str(&v)?;
                let state: State = swap.into();
                Ok((StateKey((swap_id, timestamp)), state))
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

        tracing::info!("uploading {} states", self.db.get_all_states().await?.len());

        //states table
        for (swap_id, state, timestamp) in self.db.get_all_states().await? {
            if self.eigensync_handle.write().await.get_document_state().expect("Eigensync document should be present").states.contains_key(&StateKey((swap_id, timestamp))) {
                tracing::info!("state already exists");
                continue;
            }

            self.eigensync_handle.write().await.modify(|document| {
                document.states.insert(StateKey((swap_id, timestamp)), state);
                Ok(())
            }).await?;

            //peers table
            if let Ok(peer_id) = self.db.get_peer_id(swap_id).await {
                self.eigensync_handle.write().await.modify(|document| {
                    document.peers.insert(swap_id, peer_id);
                    Ok(())
                }).await?;
            }

            if let Ok(address_pool) = self.db.get_monero_address_pool(swap_id).await {
                for labeled in address_pool.iter() {
                    let address_opt_str = labeled.address().map(|a| a.to_string());
                    let percentage = labeled.percentage();
                    let label = labeled.label().to_string();
                    self.eigensync_handle.write().await.modify(|document| {
                        document.monero_addresses.insert(MoneroAddressKey((swap_id, address_opt_str)), (percentage, label));
                        Ok(())
                    }).await?;
                }
            }

            if let Ok(proof) = self.db.get_buffered_transfer_proof(swap_id).await {
                if let Some(proof) = proof {
                    self.eigensync_handle.write().await.modify(|document| {
                        document.buffered_transfer_proofs.insert(swap_id, proof);
                        Ok(())
                    }).await?;
                }
            }
        }

        //peer_addresses table
        for (peer_id, addresses) in self.db.get_all_peer_addresses().await? {
            self.eigensync_handle.write().await.modify(|document| {
                for address in addresses {
                    document.peer_addresses.insert(PeerAddressesKey((peer_id, address)), ());
                }
                Ok(())
            }).await?;
        }

        //tracing::info!("Uploaded {} states", self.db.get_all_states().await?.len());

        Ok(())
    }

    pub async fn download_states(&self) -> anyhow::Result<()> {
        // get from document -> write into db
        let document = self.eigensync_handle.write().await.get_document_state().expect("Eigensync document should be present");
        
        tracing::info!("Document has {} states", document.states.len());

        // States table
        let document_states: HashSet<StateKey> = document.states.keys().cloned().collect();
        let document_states_len = document_states.len();
        let db_states = self.db.get_all_states().await?;

        let mut document_states = document_states.into_iter().collect::<Vec<_>>();
        document_states.sort_by_key(|state_key| state_key.0.1);

        for state_key in document_states {
            let (swap_id, timestamp) = (state_key.0.0, state_key.0.1);

            let document_state: State = document
                .states
                .get(&state_key)
                .ok_or_else(|| anyhow!("State not found for key"))?
                .clone();

            if db_states.iter().any(|(_, _, db_timestamp)| db_timestamp == &timestamp) {
                tracing::info!(?timestamp, "state already exists");
                continue;
            }

            let swap_uuid = swap_id;

            tracing::info!("inserting existing state");
            if let Err(e) = self.db.insert_existing_state(swap_uuid, document_state, timestamp).await {
                tracing::error!("Error inserting existing state: {:?}", e);
            }
        }

        //peer_addresses table
        let document_peer_addresses: HashSet<PeerAddressesKey> = document.peer_addresses.keys().cloned().collect();
        let db_peer_addresses = self.db.get_all_peer_addresses().await?;

        for peer_address_key in document_peer_addresses {
            let (peer_id, address) = (peer_address_key.0.0, peer_address_key.0.1);

            if db_peer_addresses.iter().any(|(p, a)| p == &peer_id && a.contains(&address)) {
                tracing::info!("peer address already exists");
                continue;
            }

            tracing::info!("inserting existing peer address");
            self.db.insert_address(peer_id, address).await?;
        }

        //peers table
        let document_peers: HashSet<Uuid> = document.peers.keys().cloned().collect();
        //let db_peers = self.db.get_all_peer_addresses().await?;

        for swap_id in document_peers {
            tracing::info!("Downloading peer: {:?}", swap_id);
            let document_peer = document
                .peers
                .get(&swap_id)
                .ok_or_else(|| anyhow!("Peer not found for key"))?
                .clone();

            tracing::info!("Document peer: {:?}", document_peer);

            if let Ok(peer_id) = self.db.get_peer_id(swap_id).await {
                if peer_id == document_peer {
                    tracing::info!("peer already exists");
                    continue;
                }
            }

            tracing::info!(%swap_id, %document_peer, "inserting existing peer");
            self.db.insert_peer_id(swap_id, document_peer).await?;
        }

        tracing::info!("Downloaded peers: {:?}", document.peers);

        //monero_addresses table
        let document_monero_addresses: HashSet<MoneroAddressKey> = document.monero_addresses.keys().cloned().collect();
        let db_monero_addresses = self.db.get_monero_addresses().await?;

        for monero_address_key in document_monero_addresses {
            let (swap_id, address) = (monero_address_key.0.0.clone(), monero_address_key.0.1.clone());

            // Check if the address exists in the database
            let address_exists = if let Some(addr_str) = &address {
                db_monero_addresses.iter().any(|db_addr| db_addr.to_string() == *addr_str)
            } else {
                false // No address to check
            };

            if address_exists {
                tracing::info!("monero address already exists");
                continue;
            }

            // Get the percentage and label from the document
            let (percentage, label) = document.monero_addresses.get(&monero_address_key)
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
                tracing::info!("buffered transfer proof already exists");
                continue;
            }
            
            self.db.insert_buffered_transfer_proof(swap_id, document_buffered_transfer_proof).await?;
        }
        
        tracing::info!("Downloaded {} states", document_states_len);
    
        Ok(())
    }
}