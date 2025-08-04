use std::{collections::HashMap, ops::Deref, str::FromStr, time::Duration};

use anyhow::{Context, Result};
use automerge::{ActorId, AutoCommit, Change};
use autosurgeon::{hydrate, reconcile, Hydrate, Reconcile};
use eigensync::protocol::{client, Behaviour, BehaviourEvent, Request, Response, SerializedChange};
use libp2p::{
    futures::StreamExt,
    identity, noise, request_response,
    swarm::{self, SwarmEvent},
    tcp, yamux, Multiaddr, PeerId, Swarm, SwarmBuilder,
};
use uuid::Uuid;

pub struct Database {
    document: AutoCommit,
    state: State,
}

#[derive(Debug, Clone, Reconcile, Hydrate, PartialEq)]
pub struct State {
    swaps: Vec<Swap>,
}

#[derive(Debug, Clone, Reconcile, Hydrate, PartialEq)]
pub struct Swap {
    #[key]
    pub id: Uuid,
    pub amount: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut db = Database::new();

    db.add_swap(Swap {
        id: Uuid::new_v4(),
        amount: 300,
    })
    .unwrap();

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

    let server_peer_id = PeerId::from_str("12D3KooWQsAFHUm32ThqfQRJhtcc57qqkYckSu8JkMsbGKkwTS6p")?;
    let server_addr = Multiaddr::from_str("/ip4/127.0.0.1/tcp/3333")?;

    swarm.add_peer_address(server_peer_id.clone(), server_addr.clone());
    println!("Dialing /ip4/127.0.0.1/tcp/3333");

    swarm.dial(server_addr).context("Failed to dial")?;

    let our_peer_id = *swarm.local_peer_id();

    loop {
        tokio::select! {
            event = swarm.select_next_some() => handle_event(event, server_peer_id, &mut swarm, &mut db).await?
        }
    }
}

async fn handle_event(
    event: SwarmEvent<BehaviourEvent>,
    server_id: PeerId,
    swarm: &mut Swarm<Behaviour>,
    db: &mut Database,
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
                    eprintln!("Swaps before: {:?}", db.state.swaps);
                    db.add_changes(changes.into_iter().map(|c| c.into()).collect())
                        .expect("Failed to add changes");
                    eprintln!("Swaps after: {:?}", db.state.swaps);
                }
                Response::ChangesAdded => {
                    eprintln!("Changes added to server");
                }
                Response::Error { reason } => {
                    eprintln!("Server replied with error: {}", reason);
                }
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

            tokio::time::sleep(Duration::from_secs(1)).await;
            swarm
                .dial(Multiaddr::from_str("/ip4/127.0.0.1/tcp/3333")?)
                .context("Failed to dial")?;
        }
        SwarmEvent::ConnectionEstablished { peer_id, .. } => {
            eprintln!("Connected to peer, sending request");
            let changes: Vec<_> = db.get_changes().into_iter().map(|c| c.into()).collect();
            eprintln!("Number of current changes: {}", changes.len());
            swarm
                .behaviour_mut()
                .send_request(&peer_id, Request::GetChanges { changes });
            eprintln!("Swaps: {:?}", db.state.swaps);
        }
        other => eprintln!("Received event: {:?}", other),
    })
}

impl Database {
    fn new() -> Self {
        let mut document = AutoCommit::new().with_actor(ActorId::random());

        let state = State { swaps: vec![] };

        reconcile(&mut document, &state)
            .context("Failed to reconcile")
            .unwrap();

        Self { document, state }
    }

    fn get_changes(&mut self) -> Vec<Change> {
        self.document
            .get_changes(&[])
            .iter()
            .map(|c| (*c).clone())
            .collect()
    }

    fn add_changes(&mut self, changes: Vec<Change>) -> anyhow::Result<()> {
        eprintln!("Number of changes to add: {}", changes.len());

        let mut server_doc = self.document.fork();
        server_doc
            .apply_changes(changes)
            .context("Failed to apply changes")?;

        // Make sure server state is valid
        let _: State = hydrate(&server_doc).context("Couldn't hydrate doc into state")?;

        self.document
            .merge(&mut server_doc)
            .context("Failed to merge")?;

        self.state = hydrate(&self.document).context("Couldn't hydrate doc into state")?;

        Ok(())
    }

    fn add_swap(&mut self, swap: Swap) -> anyhow::Result<()> {
        self.state.swaps.push(swap);

        reconcile(&mut self.document, &self.state).context("Failed to reconcile")?;

        Ok(())
    }
}
