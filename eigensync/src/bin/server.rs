use std::{collections::HashMap, path::PathBuf, str::FromStr, time::Duration};

use anyhow::{Context};
use directories_next::ProjectDirs;
use eigensync::protocol::{server, Behaviour, BehaviourEvent, Response, SerializedChange, ServerRequest};
use libp2p::{
    futures::StreamExt, identity, noise, request_response, swarm::SwarmEvent, tcp, yamux, Multiaddr, PeerId, Swarm, SwarmBuilder
};
use autosurgeon::{Hydrate, Reconcile};
use tracing_subscriber::EnvFilter;

use clap::Parser;

#[derive(Parser)]
struct Cli {
    #[arg(long)]
    pub data_dir: PathBuf
}

use eigensync::database::Database;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing with info level
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new("info"))
        .init();

    let data_dir = Cli::parse().data_dir;

    let db = Database::new(data_dir).await?;
    
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

    swarm.listen_on(multiaddr.clone())?;

    tracing::info!(
        "Listening on {}/p2p/{}",
        multiaddr,
        swarm.local_peer_id()
    );

    loop {
        tokio::select! {
            event = swarm.select_next_some() => handle_event(&mut swarm, event, &db).await?
        }
    }
}

async fn handle_event(swarm: &mut Swarm<Behaviour>, event: SwarmEvent<BehaviourEvent>, db: &Database) -> anyhow::Result<()> {

    match event {
        SwarmEvent::Behaviour(BehaviourEvent::Sync(request_response::Event::Message {
            peer,
            message,
        })) => {
            match message {
                request_response::Message::Request { request, channel, .. } => {

                    match request {
                        ServerRequest::UploadChangesToServer { changes } => {
                            //let saved_changed_of_peer = global_changes.entry(peer).or_insert(Vec::new());
                            let saved_changed_of_peer = db.get_peer_changes(peer).await?;

                            // Saved all changes the client sent us but we don't have yet stored for him
                            let changes_clone = changes.clone();

                            tracing::info!("Received {} changes from client", changes.len());

                            let uploaded_new_changes: Vec<_> = changes.into_iter().filter(|c| !saved_changed_of_peer.contains(c)).collect();
                            
                            db.insert_peer_changes(peer, uploaded_new_changes).await?;

                            // Check which changes the client is missing
                            let changes_client_is_missing: Vec<_> = db.get_peer_changes(peer).await?.iter().filter(|c| !changes_clone.contains(c)).cloned().collect();

                            tracing::info!("Sending {} changes to client", changes_client_is_missing.len());

                            // Send the changes the client is missing to the client
                            let response = Response::NewChanges { changes: changes_client_is_missing };
                            swarm.behaviour_mut().send_response(channel, response).expect("Failed to send response");
                        }
                    }
                }
                request_response::Message::Response { request_id, .. } => tracing::info!("Received response for request of id {:?}", request_id),
            }
        }
        SwarmEvent::ConnectionEstablished { peer_id, .. } => {
            tracing::info!("Connection established with peer: {:?}", peer_id);
        }
        SwarmEvent::ConnectionClosed { peer_id, .. } => {
            tracing::info!("Connection closed with peer: {:?}", peer_id);
        }
        other => {
            tracing::info!("Received event: {:?}", other);
        },
    };
    Ok(())}
