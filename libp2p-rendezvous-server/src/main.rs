use anyhow::{Context, Result};
use futures::{AsyncRead, AsyncWrite, StreamExt};
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::transport::Boxed;
use libp2p::core::upgrade::Version;
use libp2p::identity::ed25519;
use libp2p::noise;
use libp2p::rendezvous::server::Behaviour;
use libp2p::swarm::SwarmEvent;
use libp2p::tcp;
use libp2p::yamux;
use libp2p::{dns, SwarmBuilder};
use libp2p::{identity, rendezvous, Multiaddr, PeerId, Swarm, Transport};
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use structopt::StructOpt;
use tokio::fs;
use tokio::fs::{DirBuilder, OpenOptions};
use tokio::io::AsyncWriteExt;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::FmtSubscriber;

#[derive(Debug, StructOpt)]
struct Cli {
    /// Path to the file that contains the secret key of the rendezvous server's
    /// identity keypair
    /// If the file does not exist, a new secret key will be generated and saved to the file
    #[structopt(long, default_value = "rendezvous-server-secret.key")]
    secret_file: PathBuf,

    /// Port used for listening on TCP (default)
    #[structopt(long, default_value = "8888")]
    listen_tcp: u16,

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

    init_tracing(LevelFilter::INFO, cli.json, cli.no_timestamp);

    let secret_key = load_secret_key_from_file(&cli.secret_file).await?;

    let identity = identity::Keypair::from(ed25519::Keypair::from(secret_key));

    let mut swarm = create_swarm(identity)?;

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

fn init_tracing(level: LevelFilter, json_format: bool, no_timestamp: bool) {
    if level == LevelFilter::OFF {
        return;
    }

    let is_terminal = atty::is(atty::Stream::Stderr);

    let builder = FmtSubscriber::builder()
        .with_env_filter(format!("rendezvous_server={}", level))
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

fn create_swarm(identity: identity::Keypair) -> Result<Swarm<Behaviour>> {
    let transport = create_transport(&identity).context("Failed to create transport")?;
    let rendezvous = rendezvous::server::Behaviour::new(rendezvous::server::Config::default());

    let swarm = SwarmBuilder::with_existing_identity(identity)
        .with_tokio()
        .with_other_transport(|_| transport)?
        .with_behaviour(|_| rendezvous)?
        .build();

    Ok(swarm)
}

fn create_transport(identity: &identity::Keypair) -> Result<Boxed<(PeerId, StreamMuxerBox)>> {
    let tcp = tcp::tokio::Transport::new(tcp::Config::new().nodelay(true));
    let tcp_with_dns = dns::tokio::Transport::system(tcp)?;

    let transport = authenticate_and_multiplex(tcp_with_dns.boxed(), &identity).unwrap();

    Ok(transport)
}

fn authenticate_and_multiplex<T>(
    transport: Boxed<T>,
    identity: &identity::Keypair,
) -> Result<Boxed<(PeerId, StreamMuxerBox)>>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let noise_config = noise::Config::new(identity).unwrap();

    let transport = transport
        .upgrade(Version::V1)
        .authenticate(noise_config)
        .multiplex(yamux::Config::default())
        .timeout(Duration::from_secs(20))
        .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
        .boxed();

    Ok(transport)
}

struct Addresses<'a>(&'a [Multiaddr]);

// Prints an array of multiaddresses as a comma seperated string
impl fmt::Display for Addresses<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let display = self
            .0
            .iter()
            .map(|addr| addr.to_string())
            .collect::<Vec<String>>()
            .join(",");
        write!(f, "{}", display)
    }
}
