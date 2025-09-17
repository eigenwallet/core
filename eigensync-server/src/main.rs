use std::{fs::{self, File}, io::Write, path::{Path, PathBuf}, str::FromStr, time::Duration};

use anyhow::{Context};
use eigensync_protocol::{server, Behaviour, BehaviourEvent, Response, ServerRequest};
use libp2p::{
    futures::StreamExt, identity::{self, ed25519}, noise, request_response, swarm::SwarmEvent, tcp, yamux, Multiaddr, Swarm, SwarmBuilder
};
use tracing_subscriber::EnvFilter;

use anyhow::{Result};

use libp2p::PeerId;
use sqlx::{sqlite::{SqliteConnectOptions, SqlitePoolOptions}, SqlitePool};
use tracing::info;

use eigensync_protocol::EncryptedChange;

#[derive(Clone)]
pub struct Database {
    pub pool: SqlitePool,
}

impl Database {
    pub async fn new(data_dir: PathBuf) -> Result<Self> {
        if !data_dir.exists() {
            std::fs::create_dir_all(&data_dir)?;
            info!(data_dir = %data_dir.display(), "Created server database directory");
        }

        let db_path = data_dir.join("changes");
        let connect_options = SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true);

        let pool = SqlitePoolOptions::new()
            .connect_with(connect_options)
            .await?;

        let db = Self { pool };
        db.migrate().await?;

        Ok(db)
    }

    async fn migrate(&self) -> Result<()> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;
        info!("Server database migration completed");
        Ok(())
    }

    pub async fn get_peer_changes(&self, peer_id: PeerId) -> Result<Vec<EncryptedChange>> {
        let peer_id = peer_id.to_string();
        
        let rows = sqlx::query!(
            r#"
            SELECT change
            FROM change
            WHERE peer_id = ?
            ORDER BY id DESC
            "#,
            peer_id
        )
        .fetch_all(&self.pool)
        .await?;

        let changes = rows.iter().map(|row| EncryptedChange::new(row.change.clone())).collect();

        Ok(changes)
    }

    pub async fn insert_peer_changes(&self, peer_id: PeerId, changes: Vec<EncryptedChange>) -> Result<()> {
        let peer_id = peer_id.to_string();
    
        for change in changes {
            let serialized = change.to_bytes();
            sqlx::query!(
                r#"
                INSERT or IGNORE INTO change (peer_id, change)
                VALUES (?, ?)
                "#,
                peer_id,
                serialized
            )
            .execute(&self.pool)
            .await?;
        }
    
        Ok(())
    }
}

use clap::Parser;

#[derive(Parser)]
struct Cli {
    #[arg(long)]
    pub data_dir: PathBuf
}

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
    Ok(())
}
