use std::{collections::HashMap, marker::PhantomData, str::FromStr, sync::OnceLock, time::Duration};

use anyhow::{Context, Result};
use automerge::{ActorId, AutoCommit, Change, ScalarValue};
use eigensync::protocol::{server, Behaviour, BehaviourEvent, Request, Response, SerializedChange};
use libp2p::{
    futures::StreamExt, identity, noise, request_response::{self, ResponseChannel}, swarm::SwarmEvent, tcp, yamux, Multiaddr, PeerId, Swarm, SwarmBuilder
};
use autosurgeon::{hydrate, reconcile, Hydrate, Reconcile};

#[derive(Debug, Clone, Reconcile, Hydrate, PartialEq, Default)]
struct Foo {
    bar: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let multiaddr = Multiaddr::from_str("/ip4/127.0.0.1/tcp/3333")?;

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

    swarm.listen_on(multiaddr)?;

    println!(
        "Listening on /ip4/127.0.0.1/tcp/3333/p2p/{}",
        swarm.local_peer_id()
    );

    loop {
        tokio::select! {
            event = swarm.select_next_some() => handle_event(&mut swarm, event).await
        }
    }
}

async fn handle_event(swarm: &mut Swarm<Behaviour>, event: SwarmEvent<BehaviourEvent>) {
    match event {
        SwarmEvent::Behaviour(BehaviourEvent::Sync(request_response::Event::Message {
            peer,
            message,
        })) => {
            match message {
                request_response::Message::Request { request, channel, .. } => {
                    eprintln!("Received request from client {:?}", peer);

                    match request {
                        Request::UploadChangesToServer { changes } => {
                            swarm.behaviour_mut().send_response(channel, Response::Error { reason: "Not implemented".to_string() }).unwrap();
                        }
                    }
                }
                request_response::Message::Response { request_id, .. } => eprintln!("Received response for request of id {:?}", request_id),
            }
        }
        SwarmEvent::ConnectionEstablished { peer_id, .. } => {
            eprintln!("Connection established with peer: {:?}", peer_id);
        }
        SwarmEvent::ConnectionClosed { peer_id, .. } => {
            eprintln!("Connection closed with peer: {:?}", peer_id);
        }
        other => eprintln!("Received event: {:?}", other),
    }
}