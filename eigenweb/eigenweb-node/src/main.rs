use anyhow::{Context, Result};
use futures::StreamExt;
use libp2p::identity::{self, ed25519};
use libp2p::rendezvous;
use libp2p::swarm::SwarmEvent;
use std::path::{Path, PathBuf};
use structopt::StructOpt;
use tokio::fs;
use tokio::fs::{DirBuilder, OpenOptions};
use tokio::io::AsyncWriteExt;
use tracing_subscriber::filter::LevelFilter;

use crate::swarm::{create_swarm, create_swarm_with_onion, Addresses};
use crate::tracing_util::init_tracing;

pub mod swarm;
pub mod tracing_util;

#[derive(Debug, StructOpt)]
struct Cli {
    /// Path to the file that contains the secret key of the rendezvous server's
    /// identity keypair
    /// If the file does not exist, a new secret key will be generated and saved to the file
    #[structopt(long, default_value = "./rendezvous-node-secret.key")]
    secret_file: PathBuf,

    /// Port used for listening on TCP (default)
    #[structopt(long, default_value = "8888")]
    listen_tcp: u16,

    /// Enable listening on Tor onion service
    #[structopt(long)]
    no_onion: bool,

    /// Port for the onion service (only used if --onion is enabled)
    #[structopt(long, default_value = "8888")]
    onion_port: u16,

    /// Format logs as JSON
    #[structopt(long)]
    json: bool,

    /// Don't include timestamp in logs. Useful if captured logs already get
    /// timestamped, e.g. through journald.
    #[structopt(long)]
    no_timestamp: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::from_args();

    init_tracing(LevelFilter::TRACE, cli.json, cli.no_timestamp);

    let secret_key = load_secret_key_from_file(&cli.secret_file).await?;

    let identity = identity::Keypair::from(ed25519::Keypair::from(secret_key));

    let mut swarm = if cli.no_onion {
        create_swarm(identity)?
    } else {
        create_swarm_with_onion(identity, cli.onion_port).await?
    };

    tracing::info!(peer_id=%swarm.local_peer_id(), "Rendezvous server peer id");

    swarm
        .listen_on(
            format!("/ip4/0.0.0.0/tcp/{}", cli.listen_tcp)
                .parse()
                .expect("static string is valid MultiAddress"),
        )
        .context("Failed to initialize listener")?;

    loop {
        match swarm.select_next_some().await {
            SwarmEvent::Behaviour(rendezvous::server::Event::PeerRegistered {
                peer,
                registration,
            }) => {
                tracing::info!(%peer, namespace=%registration.namespace, addresses=?registration.record.addresses(), ttl=registration.ttl,  "Peer registered");
            }
            SwarmEvent::Behaviour(rendezvous::server::Event::PeerNotRegistered {
                peer,
                namespace,
                error,
            }) => {
                tracing::info!(%peer, %namespace, ?error, "Peer failed to register");
            }
            SwarmEvent::Behaviour(rendezvous::server::Event::RegistrationExpired(registration)) => {
                tracing::info!(peer=%registration.record.peer_id(), namespace=%registration.namespace, addresses=%Addresses(registration.record.addresses()), ttl=registration.ttl, "Registration expired");
            }
            SwarmEvent::Behaviour(rendezvous::server::Event::PeerUnregistered {
                peer,
                namespace,
            }) => {
                tracing::info!(%peer, %namespace, "Peer unregistered");
            }
            SwarmEvent::Behaviour(rendezvous::server::Event::DiscoverServed {
                enquirer, ..
            }) => {
                tracing::info!(peer=%enquirer, "Discovery served");
            }
            SwarmEvent::NewListenAddr { address, .. } => {
                tracing::info!(%address, "New listening address reported");
            }
            other => {
                tracing::debug!(?other, "Unhandled event");
            }
        }
    }
}

async fn load_secret_key_from_file(path: impl AsRef<Path>) -> Result<ed25519::SecretKey> {
    let path = path.as_ref();

    match fs::read(path).await {
        Ok(bytes) => {
            // File exists, try to load the secret key
            let secret_key = ed25519::SecretKey::try_from_bytes(bytes)?;
            Ok(secret_key)
        }
        Err(_) => {
            // File doesn't exist, generate a new secret key
            tracing::info!(
                "Secret file not found at {}, generating new key",
                path.display()
            );
            let secret_key = ed25519::SecretKey::generate();

            // Save the new key to file
            write_secret_key_to_file(&secret_key, path.to_path_buf()).await?;
            tracing::info!("New secret key saved to {}", path.display());

            Ok(secret_key)
        }
    }
}

async fn write_secret_key_to_file(secret_key: &ed25519::SecretKey, path: PathBuf) -> Result<()> {
    if let Some(parent) = path.parent() {
        DirBuilder::new()
            .recursive(true)
            .create(parent)
            .await
            .with_context(|| {
                format!(
                    "Could not create directory for secret file: {}",
                    parent.display()
                )
            })?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .await
        .with_context(|| format!("Could not generate secret file at {}", path.display()))?;

    file.write_all(secret_key.as_ref()).await?;

    Ok(())
}
