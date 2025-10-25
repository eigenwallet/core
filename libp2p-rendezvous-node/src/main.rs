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
use tracing::level_filters::LevelFilter;
use tracing_subscriber::FmtSubscriber;

use crate::swarm::{create_swarm, create_swarm_with_onion, Addresses};

pub mod behaviour;
pub mod swarm;

#[derive(Debug, StructOpt)]
struct Cli {
    /// Path to the file that contains the secret key of the rendezvous server's
    /// identity keypair
    /// If the file does not exist, a new secret key will be generated and saved to the file
    #[structopt(long, default_value = "./rendezvous-server-secret.key")]
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

    let rendezvous_addrs = swap_env::defaults::default_rendezvous_points();

    let mut swarm = if cli.no_onion {
        create_swarm(identity, rendezvous_addrs)?
    } else {
        create_swarm_with_onion(identity, cli.onion_port, rendezvous_addrs).await?
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
                rendezvous::client::Event::Registered {
                    rendezvous_node,
                    ttl,
                    namespace,
                },
            )) => {
                tracing::info!(%rendezvous_node, %namespace, ttl, "Registered at rendezvous point");
            }
            SwarmEvent::Behaviour(behaviour::BehaviourEvent::Register(
                rendezvous::client::Event::RegisterFailed {
                    rendezvous_node,
                    namespace,
                    error,
                },
            )) => {
                tracing::warn!(%rendezvous_node, %namespace, ?error, "Failed to register at rendezvous point");
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

fn init_tracing(level: LevelFilter, json_format: bool, no_timestamp: bool) {
    if level == LevelFilter::OFF {
        return;
    }

    let is_terminal = atty::is(atty::Stream::Stderr);

    let builder = FmtSubscriber::builder()
        .with_env_filter(format!(
            "rendezvous_server={},\
                 swap_p2p={},\
                 libp2p={},\
                 libp2p_allow_block_list={},\
                 libp2p_connection_limits={},\
                 libp2p_core={},\
                 libp2p_dns={},\
                 libp2p_identity={},\
                 libp2p_noise={},\
                 libp2p_ping={},\
                 libp2p_rendezvous={},\
                 libp2p_request_response={},\
                 libp2p_swarm={},\
                 libp2p_tcp={},\
                 libp2p_tls={},\
                 libp2p_tor={},\
                 libp2p_websocket={},\
                 libp2p_yamux={}",
            level,
            level,
            level,
            level,
            level,
            level,
            level,
            level,
            level,
            level,
            level,
            level,
            level,
            level,
            level,
            level,
            level,
            level
        ))
        .with_writer(std::io::stderr)
        .with_ansi(is_terminal)
        .with_target(false);

    if json_format {
        builder.json().init();
        return;
    }

    if no_timestamp {
        builder.without_time().init();
        return;
    }
    builder.init();
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
