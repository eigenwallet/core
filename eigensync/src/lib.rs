use std::{marker::PhantomData, sync::OnceLock, time::Duration};
pub mod protocol;

use anyhow::Context;
//pub mod protocol;
use libp2p::{
    futures::StreamExt, 
    identity, noise, request_response, 
    swarm::{NetworkBehaviour, SwarmEvent}, 
    tcp, yamux, Multiaddr, PeerId, Swarm, SwarmBuilder
};
use serde::{Deserialize, Serialize};
use tokio::{sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender}, task};
//use crate::protocol::Request;
use automerge::{ActorId, AutoCommit, Change};
use autosurgeon::{hydrate, reconcile, Hydrate, Reconcile};

use crate::protocol::{client, Behaviour, BehaviourEvent, Request, Response};

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

// #[derive(Clone, Debug, Serialize, Deserialize)]
// pub enum Request {
//     UploadChangesToServer {
//         changes: Vec<u8>
//     }
// }

// #[derive(Clone, Debug, Serialize, Deserialize)]
// enum Response {
//     GetChangesFromServer
// }

// #[derive(NetworkBehaviour)]
// pub struct Behaviour {
//     sync: SyncBehaviour,
// }


pub type SyncBehaviour = request_response::cbor::Behaviour<Request, Response>;

pub struct SyncLoop {
    receiver: UnboundedReceiver<Request>,
    swarm: Swarm<Behaviour>
}

impl SyncLoop {
    pub async fn new(receiver: UnboundedReceiver<Request>, swarm: Swarm<Behaviour>) -> Self {
        
        Self { receiver, swarm}
    }
    
    pub fn sync_with_server(&mut self, request: Request) {
        match &request {
            Request::UploadChangesToServer { changes: _changes } => {
                let server = self.swarm.connected_peers().next().copied();
                if let Some(server) = server {
                    self.swarm.behaviour_mut().send_request(&server, request);
                }
            },
        }
        
    }
    
    pub async fn run(&mut self, server_id: PeerId) -> anyhow::Result<()> {
        loop {
            tokio::select! {
                event = self.swarm.select_next_some() => handle_event(event, server_id, &mut self.swarm).await?,
                request_from_handle = self.receiver.recv() => {
                    if let Some(request) = request_from_handle {
                        self.sync_with_server(request);
                    }
                }
            }
        }
        
        // loop {
        //     if self.receiver.recv().await {
        //         // ..
        //     }
        // }
    }
}

pub async fn handle_event(
    event: SwarmEvent<BehaviourEvent>,
    server_id: PeerId,
    swarm: &mut Swarm<Behaviour>,
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
                Response::ChangesAdded => {
                    println!("Got changes from server");
                },
                Response::NewChanges { changes } => {
                    println!("Got new changes")
                },
                Response::Error { reason } => {
                    println!("Error")
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
            error,
            ..
        })) => {
            eprintln!("Outbound failure: {:?}", error);

            // tokio::time::sleep(Duration::from_secs(1)).await;
            // swarm
            //     .dial(Multiaddr::from_str("/ip4/127.0.0.1/tcp/3333")?)
            //     .context("Failed to dial")?;
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

pub struct Eigensync<T: Reconcile + Hydrate> {
    pub document: AutoCommit,
    sender: UnboundedSender<Request>,
    _marker: PhantomData<T>,
}

impl<T: Reconcile + Hydrate> Eigensync<T> {
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

        let document = get_init_autocommit();

        Ok(Self { document, _marker: PhantomData , sender})
    }

    pub fn get_changes(&mut self) -> Vec<Change> {
        self.document
            .get_changes(&[])
            .iter()
            .map(|c| (*c).clone())
            .collect()
    }

    pub fn send_update(&mut self, changes: Vec<Change>) {
        let serialized_changes: Vec<u8> = changes.into_iter().flat_map(|mut c| c.bytes().to_vec()).collect();
        self.sender.send(Request::UploadChangesToServer { changes: serialized_changes }).unwrap();
    }

    pub async fn save_and_sync(&mut self, value: &T) -> anyhow::Result<()> {
        let new_changes = self.get_changes();

        let mut new_doc = self.document.fork();

        new_doc
            .apply_changes(new_changes.clone())
            .context("Failed to apply changes")?;

        self.document
            .merge(&mut new_doc)
            .context("Failed to merge")?;

        self.send_update(new_changes);

        Ok(())
    }
}