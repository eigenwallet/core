use std::{collections::HashMap, marker::PhantomData, sync::OnceLock, time::Duration};
pub mod protocol;

use anyhow::Context;
//pub mod protocol;
use libp2p::{
    futures::StreamExt, 
    identity, noise, request_response::{self, OutboundRequestId}, 
    swarm::SwarmEvent, 
    tcp, yamux, Multiaddr, PeerId, Swarm, SwarmBuilder
};
use tokio::{sync::{mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender}, oneshot}, task};
//use crate::protocol::Request;
use automerge::{ActorId, AutoCommit, Change};
use autosurgeon::{hydrate, reconcile, Hydrate, Reconcile};

use crate::protocol::{client, Behaviour, BehaviourEvent, ChannelRequest, Response, SerializedChange, ServerRequest};

pub static INIT_AUTOMERGE: OnceLock<AutoCommit> = OnceLock::new();

fn get_init_autocommit() -> AutoCommit {
    INIT_AUTOMERGE.get_or_init(|| AutoCommit::load(&[133, 111, 74, 131, 88, 3, 75, 84, 1, 48, 0, 
                                                    16, 139, 82, 195, 59, 223, 191, 74, 44, 186, 
                                                    159, 214, 214, 200, 14, 181, 60, 1, 1, 0, 0, 
                                                    0, 5, 21, 7, 52, 1, 66, 2, 86, 2, 112, 2, 
                                                    127, 5, 115, 119, 97, 112, 115, 1, 127, 
                                                    0, 127, 0, 127, 0]).unwrap().with_actor(ActorId::random()))
        .clone()
}


pub type SyncBehaviour = request_response::cbor::Behaviour<ServerRequest, Response>;

pub struct SyncLoop {
    receiver: UnboundedReceiver<ChannelRequest>,
    response_map: HashMap<OutboundRequestId, oneshot::Sender<Result<Vec<SerializedChange>, String>>>,
    swarm: Swarm<Behaviour>
}

impl SyncLoop {
    pub async fn new(receiver: UnboundedReceiver<ChannelRequest>, swarm: Swarm<Behaviour>) -> Self {
        
        Self { receiver, response_map: HashMap::new(), swarm}
    }
    
    pub fn sync_with_server(&mut self, request: ChannelRequest) {
        let server_request = ServerRequest::UploadChangesToServer { changes: request.changes };
        match &server_request {
            ServerRequest::UploadChangesToServer { changes: _changes } => {
                let server = self.swarm.connected_peers().next().copied();
                if let Some(server) = server {
                    let request_id = self.swarm.behaviour_mut().send_request(&server, server_request);
                    self.response_map.insert(request_id, request.response_channel);
                }
            },
        }
    }
    
    pub async fn run(&mut self, server_id: PeerId) -> anyhow::Result<()> {
        loop {
            tokio::select! {
                event = self.swarm.select_next_some() => handle_event(event, server_id, &mut self.swarm, &mut self.response_map).await?,
                request_from_handle = self.receiver.recv() => {
                    if let Some(request) = request_from_handle {
                        self.sync_with_server(request);
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
            eprintln!("Connected to peer, sending request");
            // let changes: Vec<_> = db.get_changes().into_iter().map(|c| c.into()).collect();
            // eprintln!("Number of current changes: {}", changes.len());
            // swarm
            //     .behaviour_mut()
            //     .send_request(&peer_id, Request::GetChanges { changes });
            // eprintln!("Swaps: {:?}", db.state.swaps);
        }
        other => eprintln!("Received event: {:?}", other),
    })
}

pub struct EigensyncHandle<T: Reconcile + Hydrate + Default> {
    pub document: AutoCommit,
    sender: UnboundedSender<ChannelRequest>,
    _marker: PhantomData<T>,
}

impl<T: Reconcile + Hydrate + Default> EigensyncHandle<T> {
    pub fn new(server_addr: Multiaddr, server_id: PeerId) -> anyhow::Result<Self> {
        let keypair = identity::Keypair::ed25519_from_bytes(
            hex::decode("f77cb5d03f443675b431454acd7d45f6f032ab4d71b7ff672e662cc3e765e705").unwrap(),
        )
        .unwrap();

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

        task::spawn(async move {
            SyncLoop::new(receiver, swarm).await.run(server_id).await.unwrap();
        });

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

    pub fn modify(&mut self, f: impl FnOnce(&mut T) -> anyhow::Result<()>) -> anyhow::Result<()> {
        let mut state = hydrate(&self.document).unwrap();
        f(&mut state)?;
        self.save_updates_local(&state)?;

        println!("FIRST MODIFY WAS SUCCESSFULLY CALLED");

        Ok(())
    }

    pub async fn send_update(&mut self, changes: Vec<Change>) -> anyhow::Result<()> {
        let (sender, receiver) = oneshot::channel();

        let changes: Vec<SerializedChange> = changes.into_iter().map(SerializedChange::from).collect();
        self.sender
            .send(ChannelRequest { changes, response_channel: sender })
            .context("Failed to send changes to server")?;
        
        let new_changes_serialized = receiver.await?.map_err(|e| anyhow::anyhow!(e))?;
        let new_changes: Vec<Change> = new_changes_serialized.into_iter().map(Change::from).collect();

        println!("Applying changes {:?}", new_changes.len());

        self.document.apply_changes(new_changes).context("Failed to apply changes")?;

        Ok(())
    }

    pub fn save_updates_local(&mut self, state: &T) -> anyhow::Result<()> {
        reconcile(&mut self.document, state)
            .context("Failed to reconcile")?;
        
        Ok(())
    }

    pub async fn save_and_sync(&mut self) -> anyhow::Result<()> {
        let new_changes = self.get_changes();

        let mut new_doc = self.document.fork();

        new_doc
            .apply_changes(new_changes.clone())
            .context("Failed to apply changes")?;

        self.document
            .merge(&mut new_doc)
            .context("Failed to merge")?;

        self.send_update(new_changes).await?;

        Ok(())
    }
}