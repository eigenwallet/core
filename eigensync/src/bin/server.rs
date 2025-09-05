use std::{collections::HashMap, fs::{self, File}, io::Write, path::{Path, PathBuf}, str::FromStr, time::Duration};

use anyhow::{Context};
use directories_next::ProjectDirs;
use eigensync::protocol::{server, Behaviour, BehaviourEvent, Response, SerializedChange, ServerRequest};
use libp2p::{
    futures::StreamExt, identity::{self, ed25519}, noise, request_response, swarm::SwarmEvent, tcp, yamux, Multiaddr, PeerId, Swarm, SwarmBuilder
};
use autosurgeon::{Hydrate, Reconcile};
use tracing_subscriber::EnvFilter;
use ed25519_dalek::{Signature as DalekSignature, PublicKey as DalekPublicKey, Verifier};

use clap::Parser;

#[derive(Parser)]
struct Cli {
    #[arg(long)]
    pub data_dir: PathBuf
}

fn verify_signed_change(sc: &SerializedChange) -> anyhow::Result<()> {
    let buf = sc.to_bytes();
    anyhow::ensure!(buf.len() >= 32 + 64 + 24, "change too short");
    let (pk, rest) = buf.split_at(32);
    let (sig, body) = rest.split_at(64);
    let vk = DalekPublicKey::from_bytes(pk).map_err(|_| anyhow::anyhow!("invalid pubkey"))?;
    let sig = DalekSignature::from_bytes(sig.try_into().unwrap())?;
    vk.verify(body, &sig).map_err(|_| anyhow::anyhow!("invalid signature"))?;
    Ok(())
}

use eigensync::database::Database;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing with info level
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new("info"))
        .init();

    let data_dir = Cli::parse().data_dir;

    let db = Database::new(data_dir.clone()).await?;
    
    let multiaddr = Multiaddr::from_str("/ip4/127.0.0.1/tcp/3333")?;

    let file_path_buf = data_dir.join("seed.hex");
    let file_path = Path::new(&file_path_buf);
    let keypair = if file_path.exists() {
        let contents = fs::read_to_string(file_path)?;
        identity::Keypair::ed25519_from_bytes(hex::decode(contents)?).unwrap()
    } else {
        let secret_key = ed25519::SecretKey::generate();
        let mut file = File::create(file_path)?;
        file.write_all(hex::encode(secret_key.as_ref()).as_bytes())?;
        identity::Keypair::from(ed25519::Keypair::from(secret_key))
    };

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
                        ServerRequest::UploadChangesToServer { encrypted_changes: changes } => {
                            // Verify all changes are signed before saving
                            for (i, c) in changes.iter().enumerate() {
                                if let Err(e) = verify_signed_change(c) {
                                    let _ = swarm.behaviour_mut().send_response(channel, Response::Error { reason: format!("invalid change #{i}: {e}") });
                                    return Ok(());
                                }
                            }

                            // Existing logic
                            let saved_changed_of_peer = db.get_peer_changes(peer).await?;
                            let changes_clone = changes.clone();
                            tracing::info!("Received {} changes from client", changes.len());
                            let uploaded_new_changes: Vec<_> = changes.into_iter().filter(|c| !saved_changed_of_peer.contains(c)).collect();
                            db.insert_peer_changes(peer, uploaded_new_changes).await?;

                            let changes_client_is_missing: Vec<_> = db.get_peer_changes(peer).await?.iter().filter(|c| !changes_clone.contains(c)).cloned().collect();
                            tracing::info!("Sending {} changes to client", changes_client_is_missing.len());
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
