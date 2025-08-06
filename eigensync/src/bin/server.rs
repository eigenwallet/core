use std::{collections::HashMap, marker::PhantomData, str::FromStr, sync::OnceLock, time::Duration};

use anyhow::{Context, Result};
use automerge::{ActorId, AutoCommit, Change, ScalarValue};
use eigensync::{protocol::{server, BehaviourEvent, Request, Response, SerializedChange}, Eigensync, ServerDatabase};
use libp2p::{
    futures::StreamExt, identity, noise, request_response, swarm::SwarmEvent, tcp, yamux,
    Multiaddr, PeerId, SwarmBuilder,
};
use autosurgeon::{hydrate, reconcile, Hydrate, Reconcile};

#[derive(Debug, Clone, Reconcile, Hydrate, PartialEq, Default)]
struct Foo {
    bar: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut db: Eigensync<Foo> = Eigensync::new();

    // for constant peer id
    let keypair = identity::Keypair::ed25519_from_bytes(
        hex::decode("6c0f291615972e0cc7efa86dc19480ba9999f64b79eee98cebdfdfb1fbf1dea6").unwrap(),
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
        .with_behaviour(|_| Ok(server()))
        .context("Failed to create behaviour")?
        .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(Duration::MAX))
        .build();

    swarm.listen_on(Multiaddr::from_str("/ip4/127.0.0.1/tcp/3333")?)?;

    println!(
        "Listening on /ip4/127.0.0.1/tcp/3333/p2p/{}",
        swarm.local_peer_id()
    );

    loop {
        tokio::select! {
            event = swarm.select_next_some() => {
                match event {
                    SwarmEvent::Behaviour(BehaviourEvent::Sync(request_response::Event::Message {
                        peer,
                        message,
                    })) => {
                        match message {
                            request_response::Message::Request { request, channel, .. } => {
                                eprintln!("Received request from client {:?}", peer);

                                match request {
                                    Request::GetChanges { changes } => {
                                        eprintln!("Received GetChanges request");
                                        let changes = db.get_changes();
                                        eprintln!("Got {} new changes for client", changes.len());
                                        let serialized_changes: Vec<SerializedChange> = changes.clone().into_iter().map(|c| c.into()).collect();
                                        swarm.behaviour_mut().send_response(channel, Response::NewChanges { changes: serialized_changes }).expect("Failed to send response");
                                        eprintln!("DB size: {:?}", changes.len());
                                    }
                                    Request::AddChanges { changes } => {
                                        eprintln!("Received AddChanges request");
                                        let changes: Vec<Change> = changes.into_iter().map(|c| c.into()).collect();
                                        //db.add_changes(changes).expect("Failed to add changes");
                                        swarm.behaviour_mut().send_response(channel, Response::ChangesAdded).expect("Failed to send response");
                                        eprintln!("Changes added successfully");
                                    }
                                }
                            }
                            request_response::Message::Response { request_id, .. } => eprintln!("Received response for request of id {:?}", request_id),
                        }
                    }
                    other => eprintln!("Received event: {:?}", other),
                }
            }
        }
    }
}