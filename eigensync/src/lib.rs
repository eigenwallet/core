use std::{collections::HashMap, fmt::Debug, marker::PhantomData, sync::{Arc, OnceLock}, time::Duration};
pub mod protocol;

use anyhow::Context;
//pub mod protocol;
use libp2p::{
    futures::StreamExt, 
    identity, noise, request_response::{self, OutboundRequestId}, 
    swarm::SwarmEvent, 
    tcp, yamux, Multiaddr, PeerId, Swarm, SwarmBuilder
};
use tokio::{sync::{mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender}, oneshot, RwLock}, task};
//use crate::protocol::Request;
use automerge::{ActorId, AutoCommit, Change};
use autosurgeon::{hydrate, reconcile, Hydrate, Reconcile};
use tokio_util::task::AbortOnDropHandle;
use crate::protocol::{client, Behaviour, BehaviourEvent, ChannelRequest, Response, SerializedChange, ServerRequest};

pub type SyncBehaviour = request_response::cbor::Behaviour<ServerRequest, Response>;

pub struct SyncLoop {
    receiver: UnboundedReceiver<ChannelRequest>,
    response_map: HashMap<OutboundRequestId, oneshot::Sender<Result<Vec<SerializedChange>, String>>>,
    swarm: Swarm<Behaviour>,
    connection_established: Option<oneshot::Sender<()>>,
}

impl SyncLoop {
    pub async fn new(receiver: UnboundedReceiver<ChannelRequest>, swarm: Swarm<Behaviour>, connection_established: oneshot::Sender<()>) -> Self {
        
        Self { receiver, response_map: HashMap::new(), swarm, connection_established: Some(connection_established)}
    }
    
    pub fn sync_with_server(&mut self, request: ChannelRequest, server_id: PeerId) {
        let server_request = ServerRequest::UploadChangesToServer { changes: request.changes };
        match &server_request {
            ServerRequest::UploadChangesToServer { changes: _changes } => {
                let request_id = self.swarm.behaviour_mut().send_request(&server_id, server_request);
                self.response_map.insert(request_id, request.response_channel);
            },
        }
    }
    
    pub async fn run(&mut self, server_id: PeerId) -> anyhow::Result<()> {
        loop {
            tokio::select! {
                event = self.swarm.select_next_some() => handle_event(event, server_id, &mut self.swarm, &mut self.response_map, self.connection_established.take()).await?,
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
    server_id: PeerId,
    swarm: &mut Swarm<Behaviour>,
    response_map: &mut HashMap<OutboundRequestId, oneshot::Sender<Result<Vec<SerializedChange>, String>>>,
    mut connection_established: Option<oneshot::Sender<()>>,
) -> anyhow::Result<()> {
    Ok(match event {
        SwarmEvent::Behaviour(BehaviourEvent::Sync(request_response::Event::Message {
            peer,
            message,
        })) => match message {
            request_response::Message::Response {
                request_id,
                response,
            } => match response {
                Response::NewChanges { changes } => {
                    let sender = match response_map.remove(&request_id) {
                        Some(sender) => sender,
                        None => {
                            println!("No sender for request id {:?}", request_id);
                            return Ok(());
                        }
                    };

                    let _ = sender.send(Ok(changes));
                },
                Response::Error { reason } => {
                    let sender = match response_map.remove(&request_id) {
                        Some(sender) => sender,
                        None => {
                            println!("No sender for request id {:?}", request_id);
                            return Ok(());
                        }
                    };

                    let _ = sender.send(Err(reason.clone()));
                },
            },
            request_response::Message::Request {
                request,
                channel,
                request_id,
            } => {
                eprintln!("Received request of id {:?}", request_id);
            }
        },
        SwarmEvent::Behaviour(BehaviourEvent::Sync(request_response::Event::OutboundFailure {
            peer,
            request_id,
            error,
        })) => {
            let sender = match response_map.remove(&request_id) {
                Some(sender) => sender,
                None => {
                    println!("No sender for request id {:?}", request_id);
                    return Ok(());
                }
            };

            let _ = sender.send(Err(error.to_string()));
        }
        SwarmEvent::ConnectionEstablished { peer_id: _peer_id, .. } => {
            // send the connection established signal
            if let Some(sender) = connection_established.take() {
                let _ = sender.send(());
            }
        },
        other => eprintln!("Received event: {:?}", other),
    })
}

pub struct EigensyncHandle<T: Reconcile + Hydrate + Default + Debug> {
    pub document: AutoCommit,
    sender: UnboundedSender<ChannelRequest>,
    _marker: PhantomData<T>,
}

impl<T: Reconcile + Hydrate + Default + Debug> EigensyncHandle<T> {
    pub async fn new(server_addr: Multiaddr, server_id: PeerId, keypair: identity::Keypair) -> anyhow::Result<Self> {

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
        println!("Dialing server at {}/{}", server_addr, server_id);

        swarm.dial(server_id).context("Failed to dial")?;
        
        let (sender, receiver) = unbounded_channel();
        let (connection_ready_tx, connection_ready_rx) = oneshot::channel();

        task::spawn(async move {
            SyncLoop::new(receiver, swarm, connection_ready_tx).await.run(server_id).await.unwrap();
        });

        connection_ready_rx.await.context("Failed to establish connection")?;

        let mut document = AutoCommit::new().with_actor(ActorId::random());
        let state = T::default();
        reconcile(&mut document, &state).unwrap();

        Ok(Self { document, _marker: PhantomData , sender})
    }

    pub fn get_changes(&mut self) -> Vec<Change> {
        self.document
            .get_changes(&[])
            .iter()
            .map(|c| (*c).clone())
            .collect()
    }

    pub async fn modify(&mut self, f: impl FnOnce(&mut T) -> anyhow::Result<()>) -> anyhow::Result<()> {
        self.save_updates_local(f)?;

        let _ = self.sync_with_server().await.unwrap();

        Ok(())
    }

    pub async fn sync_with_server(&mut self) -> anyhow::Result<()> {
        let (sender, receiver) = oneshot::channel();

        let changes: Vec<SerializedChange> = self.get_changes().into_iter().map(SerializedChange::from).collect();
        self.sender
            .send(ChannelRequest { changes, response_channel: sender })
            .context("Failed to send changes to server")?;
        
        let new_changes_serialized = receiver.await?.map_err(|e| anyhow::anyhow!(e))?;
        let new_changes: Vec<Change> = new_changes_serialized.into_iter().map(Change::from).collect();

        println!("Applying changes {:?}", new_changes.len());

        for change in new_changes.clone() {
            // print the changes that are not in the document yet
            if !self.document.get_changes(&[]).contains(&&change) {
                println!("Change {:?} is not in the document yet", hydrate::<_, T>(&self.document).unwrap());
            }
        }

        self.document.apply_changes(new_changes).context("Failed to apply changes")?;

        Ok(())
    }

    pub fn save_updates_local(&mut self,  f: impl FnOnce(&mut T) -> anyhow::Result<()>) -> anyhow::Result<()> {
        let mut state = hydrate(&self.document).unwrap();
        f(&mut state)?;
        reconcile(&mut self.document, state)
            .context("Failed to reconcile")?;
        
        Ok(())
    }
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
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                let _ = handle.write().await.sync_with_server().await;
            }
        }))
    }
}