use anyhow::{Context, Result};
use futures::StreamExt;
use libp2p::identity::{self, ed25519};
use libp2p::rendezvous;
use libp2p::swarm::SwarmEvent;
use std::path::{Path, PathBuf};
use structopt::StructOpt;
use swap_p2p::protocols::rendezvous::register;
use tokio::fs;
use tokio::fs::{DirBuilder, OpenOptions};
use tokio::io::AsyncWriteExt;
use tracing::level_filters::LevelFilter;

use crate::swarm::{create_swarm, create_swarm_with_onion, Addresses};

pub mod behaviour;
pub mod swarm;
pub mod tor;
pub mod tracing_util;

#[derive(Debug, StructOpt)]
struct Cli {
    /// If the directory does not exist, it will be created
    /// Contains Tor state and LibP2P identity
    #[structopt(long, default_value = "./rendezvous-data")]
    data_dir: PathBuf,

    /// Port used for listening on TCP and onion service
    #[structopt(long, default_value = "8888")]
    port: u16,

    /// Enable listening on Tor onion service
    #[structopt(long)]
    no_onion: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::from_args();

    tracing_util::init_tracing(LevelFilter::TRACE);

    // Create data directory if it doesn't exist
    DirBuilder::new()
        .recursive(true)
        .create(&cli.data_dir)
        .await
        .with_context(|| {
            format!(
                "Could not create data directory: {}",
                cli.data_dir.display()
            )
        })?;

    let identity_file = cli.data_dir.join("identity.secret");
    let secret_key = load_secret_key_from_file(&identity_file).await?;

    let identity = identity::Keypair::from(ed25519::Keypair::from(secret_key));

    let rendezvous_addrs = swap_env::defaults::default_rendezvous_points();

    let mut swarm = if cli.no_onion {
        create_swarm(identity, rendezvous_addrs)?
    } else {
        create_swarm_with_onion(identity, cli.port, &cli.data_dir, rendezvous_addrs).await?
    };

    tracing::info!(peer_id=%swarm.local_peer_id(), "Rendezvous server peer id");

    swarm
        .listen_on(
            format!("/ip4/0.0.0.0/tcp/{}", cli.port)
                .parse()
                .expect("static string is valid MultiAddress"),
        )
        .context("Failed to initialize listener")?;

    loop {
        match swarm.select_next_some().await {
            SwarmEvent::Behaviour(behaviour::BehaviourEvent::Server(
                rendezvous::server::Event::PeerRegistered { peer, registration },
            )) => {
                tracing::info!(%peer, namespace=%registration.namespace, addresses=?registration.record.addresses(), ttl=registration.ttl,  "Peer registered");
            }
            SwarmEvent::Behaviour(behaviour::BehaviourEvent::Server(
                rendezvous::server::Event::PeerNotRegistered {
                    peer,
                    namespace,
                    error,
                },
            )) => {
                tracing::info!(%peer, %namespace, ?error, "Peer failed to register");
            }
            SwarmEvent::Behaviour(behaviour::BehaviourEvent::Server(
                rendezvous::server::Event::RegistrationExpired(registration),
            )) => {
                tracing::info!(peer=%registration.record.peer_id(), namespace=%registration.namespace, addresses=%Addresses(registration.record.addresses()), ttl=registration.ttl, "Registration expired");
            }
            SwarmEvent::Behaviour(behaviour::BehaviourEvent::Server(
                rendezvous::server::Event::PeerUnregistered { peer, namespace },
            )) => {
                tracing::info!(%peer, %namespace, "Peer unregistered");
            }
            SwarmEvent::Behaviour(behaviour::BehaviourEvent::Server(
                rendezvous::server::Event::DiscoverServed { enquirer, .. },
            )) => {
                tracing::info!(peer=%enquirer, "Discovery served");
            }
            SwarmEvent::Behaviour(behaviour::BehaviourEvent::Register(
                register::Event::Registered { peer_id },
            )) => {
                tracing::info!(%peer_id, "Registered at rendezvous point");
            }
            SwarmEvent::Behaviour(behaviour::BehaviourEvent::Register(
                register::Event::RegisterRequestFailed { peer_id, error },
            )) => {
                tracing::warn!(%peer_id, ?error, "Failed to register at rendezvous point");
            }
            SwarmEvent::Behaviour(behaviour::BehaviourEvent::Register(
                register::Event::RegisterDispatchFailed { peer_id, error },
            )) => {
                tracing::warn!(%peer_id, ?error, "Failed to dispatch register request at rendezvous point");
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
                "Identity file not found at {}, generating new key",
                path.display()
            );
            let secret_key = ed25519::SecretKey::generate();

            // Save the new key to file
            write_secret_key_to_file(&secret_key, path.to_path_buf()).await?;
            tracing::info!("New identity saved to {}", path.display());

            Ok(secret_key)
        }
    }
}

async fn write_secret_key_to_file(secret_key: &ed25519::SecretKey, path: PathBuf) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .await
        .with_context(|| format!("Could not generate identity file at {}", path.display()))?;

    file.write_all(secret_key.as_ref()).await?;

    Ok(())
}
