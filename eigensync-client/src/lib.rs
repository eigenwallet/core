use std::{collections::HashMap, fmt::Debug, marker::PhantomData, sync::Arc, time::Duration};

use anyhow::{Context};
use libp2p::{
    futures::StreamExt, 
    identity, noise, request_response::{self, OutboundRequestId}, 
    swarm::SwarmEvent, 
    tcp, yamux, Multiaddr, PeerId, Swarm, SwarmBuilder
};
use tokio::{sync::{mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender}, oneshot, RwLock}, task};
use automerge::{ActorId, AutoCommit, Change};
use autosurgeon::{hydrate, reconcile, Hydrate, Reconcile};
use tokio_util::task::AbortOnDropHandle;
use eigensync_protocol::{client, Behaviour, BehaviourEvent, Response, SerializedChange, EncryptedChange, ServerRequest};

/// High-level handle for synchronizing a typed application state with an
/// Eigensync server using Automerge and libp2p.
///
/// This handle owns an Automerge `AutoCommit` document and a client-side
/// networking loop. It provides:
/// - hydration of the CRDT into a typed `T` via `autosurgeon::hydrate`
/// - reconciliation of edits back into the CRDT via `autosurgeon::reconcile`
/// - signing, encrypting, and uploading local changes to the server
/// - downloading, verifying, decrypting, and applying remote changes
///
/// Usage outline:
/// 1) Construct with `new(server_addr, server_id, keypair, encryption_key)`. It starts networking
///    without blocking for connectivity.
/// 2) Read the current typed state with `get_document_state()`.
/// 3) Modify state with `modify(|state| { /* mutate */ Ok(()) })`, which reconciles into the CRDT.
/// 4) Call `sync_with_server().await` to push/pull changes, or wrap the handle in `Arc<RwLock<_>>`
///    and use `EigensyncHandleBackgroundSync::background_sync()` for periodic syncing.
///
/// Security notes:
/// - Changes are encrypted client-side with XChaCha20-Poly1305 using a 32-byte key.
/// - A deterministic nonce is derived from the plaintext plus key using BLAKE3 to make
///   re-encryption idempotent; do not reuse the same key across unrelated datasets.
///
/// Type parameter:
/// - `T: Reconcile + Hydrate + Default + Debug` is your typed view over the CRDT document.
///
/// Example (simplified):
/// ```ignore
/// use autosurgeon::{Hydrate, Reconcile};
/// use eigensync::EigensyncHandle;
/// use libp2p::{identity, Multiaddr, PeerId};
///
/// #[derive(Debug, Default, Hydrate, Reconcile)]
/// struct AppState {
///     // your fields
/// }
///
/// # async fn demo() -> anyhow::Result<()> {
/// let server_addr: Multiaddr = "/ip4/127.0.0.1/tcp/3333".parse()?;
/// let server_id: PeerId = "12D3KooW...".parse()?; // server's PeerId
/// let keypair = identity::Keypair::generate_ed25519();
/// let encryption_key = [0u8; 32];
///
/// let mut handle = EigensyncHandle::<AppState>::new(
///     server_addr, server_id, keypair, encryption_key,
/// ).await?;
///
/// // Read current state
/// let _state = handle.get_document_state()?;
///
/// // Make changes
/// handle.modify(|s| {
///     // mutate s
///     Ok(())
/// })?;
///
/// // Push/pull
/// handle.sync_with_server().await?;
/// # Ok(()) }
/// ```
pub struct EigensyncHandle<T: Reconcile + Hydrate + Default + Debug> {
    pub document: AutoCommit,
    sender: UnboundedSender<ChannelRequest>,
    encryption_key: [u8; 32],
    _marker: PhantomData<T>,
    connection_ready_rx: Option<oneshot::Receiver<()>>,
}

impl<T: Reconcile + Hydrate + Default + Debug> EigensyncHandle<T> {
    pub async fn new(server_addr: Multiaddr, server_id: PeerId, keypair: identity::Keypair, encryption_key: [u8; 32]) -> anyhow::Result<Self> {

        let mut swarm = SwarmBuilder::with_existing_identity(keypair)
            .with_tokio()
            .with_tcp(
                tcp::Config::default(),
                noise::Config::new,
                yamux::Config::default,
            )
            .context("Failed to create TCP transport")?
            .with_behaviour(|_| Ok(client()))
            .context("Failed to create behaviour")?
            .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(Duration::MAX))
            .build();
                
        swarm.add_peer_address(server_id.clone(), server_addr.clone());

        swarm.dial(server_id).context("Failed to dial")?;
        
        let (sender, receiver) = unbounded_channel();
        let (connection_ready_tx, connection_ready_rx) = oneshot::channel();

        task::spawn(async move {
            SyncLoop::new(receiver, swarm, connection_ready_tx).await.run(server_id).await.unwrap();
        });

        let document = AutoCommit::new().with_actor(ActorId::random());

        let handle = Self {
            document,
            _marker: PhantomData,
            sender,
            encryption_key: encryption_key.clone(),
            connection_ready_rx: Some(connection_ready_rx),
        };

        Ok(handle)
    }

    pub fn get_changes(&mut self) -> Vec<Change> {
        self.document
            .get_changes(&[])
            .iter()
            .map(|c| (*c).clone())
            .collect()
    }

    pub fn get_document_state(&mut self) -> anyhow::Result<T> {
        hydrate(&self.document).context("Failed to hydrate document")
    }

    pub async fn sync_with_server(&mut self) -> anyhow::Result<()> {
        let (sender, receiver) = oneshot::channel();
    
        let changes: Vec<SerializedChange> = self.get_changes().into_iter().map(SerializedChange::from).collect();
    
        // Encrypt each change (deterministic nonce)
        let encrypted_changes: Vec<EncryptedChange> = changes
            .iter()
            .map(|c| c.sign_and_encrypt(&self.encryption_key))
            .collect::<anyhow::Result<_>>()?;
    
        self.sender
            .send(ChannelRequest { encrypted_changes, response_channel: sender })
            .context("Failed to send changes to server")?;
    
        let new_changes_serialized = receiver.await?.map_err(|e| anyhow::anyhow!(e))?;
    
        // Decrypt as early as possible; warn and skip on failure
        let decrypted_serialized: Vec<SerializedChange> = new_changes_serialized
            .into_iter()
            .enumerate()
            .filter_map(|(i, c)| {
                match c.decrypt_and_verify(&self.encryption_key) {
                    Ok(pt) => Some(pt),
                    Err(e) => {
                        tracing::warn!("Ignoring invalid change #{}: {}", i, e);
                        None
                    }
                }
            })
            .collect();
    
        // Try to deserialize; warn and skip on failure
        let new_changes: Vec<Change> = decrypted_serialized
            .into_iter()
            .enumerate()
            .filter_map(|(i, sc)| {
                match Change::from_bytes(sc.to_bytes()) {
                    Ok(ch) => Some(ch),
                    Err(e) => {
                        tracing::warn!("Ignoring undecodable change #{}: {}", i, e);
                        None
                    }
                }
            })
            .collect();
    
        self.document.apply_changes(new_changes)?;
    
        Ok(())
    }

    pub fn modify(&mut self,  f: impl FnOnce(&mut T) -> anyhow::Result<()>) -> anyhow::Result<()> {
        let mut state = hydrate(&self.document).context("Failed to hydrate document")?;
        f(&mut state)?;
        reconcile(&mut self.document, state)
            .context("Failed to reconcile")?;
        
        Ok(())
    }
}

#[derive(Debug)]
pub struct ChannelRequest {
    pub encrypted_changes: Vec<EncryptedChange>,
    pub response_channel: oneshot::Sender<anyhow::Result<Vec<EncryptedChange>>>
}

pub struct SyncLoop {
    receiver: UnboundedReceiver<ChannelRequest>,
    response_map: HashMap<OutboundRequestId, oneshot::Sender<anyhow::Result<Vec<EncryptedChange>>>>,
    swarm: Swarm<Behaviour>,
    connection_established: Option<oneshot::Sender<()>>,
}

impl SyncLoop {
    pub async fn new(receiver: UnboundedReceiver<ChannelRequest>, swarm: Swarm<Behaviour>, connection_established: oneshot::Sender<()>) -> Self {
        
        Self { receiver, response_map: HashMap::new(), swarm, connection_established: Some(connection_established)}
    }
    
    pub fn sync_with_server(&mut self, request: ChannelRequest, server_id: PeerId) {
        let server_request = ServerRequest::UploadChangesToServer { encrypted_changes: request.encrypted_changes };
        let request_id = self.swarm.behaviour_mut().send_request(&server_id, server_request);
        self.response_map.insert(request_id, request.response_channel);
    }
    
    pub async fn run(&mut self, server_id: PeerId) -> anyhow::Result<()> {
        loop {
            tokio::select! {
                event = self.swarm.select_next_some() => {
                    if let Err(e) = handle_event(
                        event,
                        server_id,
                        &mut self.swarm,
                        &mut self.response_map,
                        self.connection_established.take()
                    ).await {
                        tracing::error!(%e, "Eigensync event handling failed");
                    }
                },
                request_from_handle = self.receiver.recv() => {
                    if let Some(request) = request_from_handle {
                        self.sync_with_server(request, server_id);
                    }
                }
            }
        }
    }
}

pub async fn handle_event(
    event: SwarmEvent<BehaviourEvent>,
    _server_id: PeerId,
    _swarm: &mut Swarm<Behaviour>,
    response_map: &mut HashMap<OutboundRequestId, oneshot::Sender<anyhow::Result<Vec<EncryptedChange>>>>,
    mut connection_established: Option<oneshot::Sender<()>>,
) -> anyhow::Result<()> {
    Ok(match event {
        SwarmEvent::Behaviour(BehaviourEvent::Sync(request_response::Event::Message {
            peer: _,
            message,
        })) => match message {
            request_response::Message::Response {
                request_id,
                response,
            } => match response {
                Response::NewChanges { changes } => {
                    let sender = response_map.remove(&request_id).context(format!("No sender for request id {:?}", request_id))?;

                    if let Err(e) = sender.send(Ok(changes)) {
                        tracing::error!("Failed to send changes to client: {:?}", e);
                    }
                },
                Response::Error { reason } => {
                    let sender = response_map.remove(&request_id).context(format!("No sender for request id {:?}", request_id))?;

                    if let Err(e) = sender.send(Err(anyhow::anyhow!(reason.clone()))) {
                        tracing::error!("Failed to send error to client: {:?}", e);
                    }
                },
            },
            request_response::Message::Request {
                request: _,
                channel: _,
                request_id: _,
            } => {
                tracing::error!("Received the request when we're the client");
            }
        },
        SwarmEvent::Behaviour(BehaviourEvent::Sync(request_response::Event::OutboundFailure {
            peer: _,
            request_id,
            error,
        })) => {
            let sender = response_map.remove(&request_id).context(format!("No sender for request id {:?}", request_id))?;

            if let Err(e) = sender.send(Err(anyhow::anyhow!(error.to_string()))) {
                tracing::error!("Failed to send error to client: {:?}", e);
            }
        }
        SwarmEvent::ConnectionEstablished { peer_id: _peer_id, .. } => {
            // send the connection established signal
            if let Some(sender) = connection_established.take() {
                if let Err(e) = sender.send(()) {
                    tracing::error!("Failed to send connection established signal to client: {:?}", e);
                }
            }
        },
        other => tracing::debug!("Received event: {:?}", other),
    })
}



pub trait EigensyncHandleBackgroundSync {
    fn background_sync(&mut self) -> AbortOnDropHandle<()>;
}

impl<T> EigensyncHandleBackgroundSync for Arc<RwLock<EigensyncHandle<T>>>
where
    T: Reconcile + Hydrate + Default + Debug + Send + Sync + 'static,
{
    fn background_sync(&mut self) -> AbortOnDropHandle<()> {
        let handle = self.clone();
        AbortOnDropHandle::new(tokio::task::spawn(async move {
            let mut seeded_default = false;
            let connection_ready_rx = {
                let mut guard = handle.write().await;
                guard.connection_ready_rx.take()
            };
            if let Some(rx) = connection_ready_rx {
                if let Err(e) = rx.await {
                    tracing::error!(%e, "Background sync failed, continuing, seeded_default: {}", seeded_default);
                    return;
                }
            }
            loop {
                println!("Background sync loop");
                // Try sync; if offline, we still proceed to seed default once
                if let Err(e) = handle.write().await.sync_with_server().await {
                    tracing::error!(%e, "Background sync failed, continuing, seeded_default: {}", seeded_default);
                    continue;
                }

                if !seeded_default {
                    println!("Seeding default eigensync state");
                    let mut guard = handle.write().await;
                    if guard.document.get_changes(&[]).is_empty() {
                        let state = T::default();
                        println!("Seeding default eigensync state, document is empty");
                        if let Err(e) = reconcile(&mut guard.document, &state) {
                            tracing::error!(error = ?e, "Failed to seed default eigensync state, continuing");
                        }
                    }
                    seeded_default = true;
                }

                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }))
    }
}