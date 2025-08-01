use std::{collections::HashMap, ops::Deref, str::FromStr, time::Duration};

use anyhow::{Context, Result};
use automerge::{AutoCommit, Change};
use autosurgeon::{reconcile, Hydrate, Reconcile};
use eigensync::{
    client,
    protocol::{BehaviourEvent, SerializedChange},
    server, Request, Response,
};
use libp2p::{
    futures::StreamExt,
    identity, noise, request_response,
    swarm::{self, SwarmEvent},
    tcp, yamux, Multiaddr, PeerId, SwarmBuilder,
};

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
    pub amount: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut db = Database::new();

    db.add_swap(Swap { amount: 100 }).unwrap();

    let mut swarm = SwarmBuilder::with_existing_identity(identity::Keypair::generate_ed25519())
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

    swarm.add_peer_address(
        PeerId::from_str("12D3KooWGq4GNHFMhWqnZJG8duJ8ZwRE4A39SxvwpiGjgPyXmJLj")?,
        Multiaddr::from_str("/ip4/127.0.0.1/tcp/3333")?,
    );
    println!("Dialing /ip4/127.0.0.1/tcp/3333");

    swarm
        .dial(Multiaddr::from_str("/ip4/127.0.0.1/tcp/3333")?)
        .context("Failed to dial")?;

    let peer_id = *swarm.local_peer_id();

    loop {
        tokio::select! {
            event = swarm.select_next_some() => {
                match event {
                    SwarmEvent::Behaviour(BehaviourEvent::Sync(request_response::Event::Message {
                        peer,
                        message,
                    })) => {
                        match message {
                            request_response::Message::Response { request_id, response } => {
                                match response {
                                    Response::NewChanges { changes } => {
                                        db.add_changes(changes.into_iter().map(|c| c.into()).collect()).expect("Failed to add changes");
                                    }
                                    Response::ChangesAdded => {
                                        eprintln!("Changes added to server");
                                    }
                                    Response::Error { reason } => {
                                        eprintln!("Server replied with error: {}", reason);
                                    }
                                }
                            }
                            request_response::Message::Request { request, channel, request_id } => {
                                eprintln!("Received request of id {:?}", request_id);
                            }
                        }
                    }
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
                    SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } => {
                        eprintln!("Connected to peer, sending request");
                        swarm.behaviour_mut().send_request(
                            &peer_id,
                            Request::AddChanges {
                                changes: db.get_changes().into_iter().map(|c| c.into()).collect(),
                            },
                        );
                    }
                    other => eprintln!("Received event: {:?}", other),
                }
            }
        }
    }
}

impl Database {
    fn new() -> Self {
        Self {
            document: AutoCommit::new(),
            state: State { swaps: vec![] },
        }
    }

    fn get_changes(&mut self) -> Vec<Change> {
        self.document
            .get_changes(&[])
            .iter()
            .map(|c| c.deref().clone())
            .collect()
    }

    fn add_changes(&mut self, changes: Vec<Change>) -> anyhow::Result<()> {
        self.document
            .apply_changes(changes)
            .context("Failed to apply changes")
    }

    fn add_swap(&mut self, swap: Swap) -> anyhow::Result<()> {
        self.state.swaps.push(swap);

        reconcile(&mut self.document, &self.state).context("Failed to reconcile")?;

        Ok(())
    }
}
