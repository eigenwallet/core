use crate::{asb, electrs, images::PINNED_GIT_REPOSITORY};
use compose_spec::Compose;
use std::{
    fmt::{self, Display},
    path::PathBuf,
};

pub const ASB_DATA_DIR: &str = "/asb-data";
pub const ASB_CONFIG_FILE: &str = "config.toml";
pub const DOCKER_COMPOSE_FILE: &str = "./docker-compose.yml";

pub struct OrchestratorInput {
    pub ports: OrchestratorPorts,
    pub networks: OrchestratorNetworks<monero::Network, bitcoin::Network>,
    pub images: OrchestratorImages<OrchestratorImage>,
    pub directories: OrchestratorDirectories,
}

pub struct OrchestratorDirectories {
    pub asb_data_dir: PathBuf,
}

#[derive(Clone)]
pub struct OrchestratorNetworks<MN: IntoFlag + Clone, BN: IntoFlag + Clone> {
    pub monero: MN,
    pub bitcoin: BN,
}

pub struct OrchestratorImages<T: IntoImageAttribute> {
    pub monerod: T,
    pub electrs: T,
    pub bitcoind: T,
    pub asb: T,
    pub asb_controller: T,
}

pub struct OrchestratorPorts {
    pub monerod_rpc: u16,
    pub bitcoind_rpc: u16,
    pub bitcoind_p2p: u16,
    pub electrs: u16,
    pub asb_libp2p: u16,
    pub asb_rpc_port: u16,
}

impl Into<OrchestratorPorts> for OrchestratorNetworks<monero::Network, bitcoin::Network> {
    fn into(self) -> OrchestratorPorts {
        match (self.monero, self.bitcoin) {
            (monero::Network::Mainnet, bitcoin::Network::Bitcoin) => OrchestratorPorts {
                monerod_rpc: 18081,
                bitcoind_rpc: 8332,
                bitcoind_p2p: 8333,
                electrs: 50001,
                asb_libp2p: 9939,
                asb_rpc_port: 9944,
            },
            (monero::Network::Stagenet, bitcoin::Network::Testnet) => OrchestratorPorts {
                monerod_rpc: 38081,
                bitcoind_rpc: 18332,
                bitcoind_p2p: 18333,
                electrs: 50001,
                asb_libp2p: 9839,
                asb_rpc_port: 9944,
            },
            _ => panic!("Unsupported Bitcoin / Monero network combination"),
        }
    }
}

impl Into<asb::Network> for OrchestratorNetworks<monero::Network, bitcoin::Network> {
    fn into(self) -> asb::Network {
        asb::Network::new(self.monero, self.bitcoin)
    }
}

impl Into<electrs::Network> for OrchestratorNetworks<monero::Network, bitcoin::Network> {
    fn into(self) -> electrs::Network {
        electrs::Network::new(self.bitcoin)
    }
}

impl OrchestratorDirectories {
    pub fn asb_config_path_inside_container(&self) -> PathBuf {
        self.asb_data_dir.join(ASB_CONFIG_FILE)
    }

    pub fn asb_config_path_on_host(&self) -> &'static str {
        // The config file is in the same directory as the docker-compose.yml file
        "./config.toml"
    }
}

/// See: https://docs.docker.com/reference/compose-file/build/#illustrative-example
#[derive(Debug, Clone)]
pub struct DockerBuildInput {
    // Usually this is the root of the Cargo workspace
    pub context: &'static str,
    // Usually this is the path to the Dockerfile
    pub dockerfile: &'static str,
}

/// Specified a docker image to use
/// The image can either be pulled from a registry or built from source
pub enum OrchestratorImage {
    Registry(String),
    Build(DockerBuildInput),
}

#[macro_export]
macro_rules! flag {
    ($flag:expr) => {
        Flag(Some($flag.to_string()))
    };
    ($flag:expr, $($args:expr),*) => {
        flag!(format!($flag, $($args),*))
    };
}

macro_rules! command {
    ($command:expr $(, $flag:expr)* $(,)?) => {
        Flags(vec![flag!($command) $(, $flag)*])
    };
}

fn build(input: OrchestratorInput) -> String {
    // Every docker compose project has a name
    // The name is prefixed to the container names
    // See: https://docs.docker.com/compose/how-tos/project-name/#set-a-project-name
    let project_name = format!(
        "{}_monero_{}_bitcoin",
        input.networks.monero.to_display(),
        input.networks.bitcoin.to_display()
    );

    let asb_config_path = PathBuf::from(ASB_DATA_DIR).join(ASB_CONFIG_FILE);
    let asb_network: asb::Network = input.networks.clone().into();

    let command_asb = command![
        "asb",
        asb_network.to_flag(),
        flag!("--config={}", asb_config_path.display()),
        flag!("start"),
        flag!("--rpc-bind-port={}", input.ports.asb_rpc_port),
        flag!("--rpc-bind-host=0.0.0.0"),
    ];

    let command_monerod = command![
        "monerod",
        input.networks.monero.to_flag(),
        flag!("--rpc-bind-ip=0.0.0.0"),
        flag!("--rpc-bind-port={}", input.ports.monerod_rpc),
        flag!("--data-dir=/monerod-data/"),
        flag!("--confirm-external-bind"),
        flag!("--restricted-rpc"),
        flag!("--non-interactive"),
        flag!("--enable-dns-blocklist"),
    ];

    let command_bitcoind = command![
        "bitcoind",
        input.networks.bitcoin.to_flag(),
        flag!("-rpcallowip=0.0.0.0/0"),
        flag!("-rpcbind=0.0.0.0:{}", input.ports.bitcoind_rpc),
        flag!("-bind=0.0.0.0:{}", input.ports.bitcoind_p2p),
        flag!("-datadir=/bitcoind-data/"),
        flag!("-dbcache=16384"),
        // These are required for electrs
        // See: See: https://github.com/romanz/electrs/blob/master/doc/config.md#bitcoind-configuration
        flag!("-server=1"),
        flag!("-prune=0"),
        flag!("-txindex=1"),
    ];

    let electrs_network: electrs::Network = input.networks.clone().into();

    let command_electrs = command![
        "electrs",
        electrs_network.to_flag(),
        flag!("--daemon-dir=/bitcoind-data/"),
        flag!("--db-dir=/electrs-data/db"),
        flag!("--daemon-rpc-addr=bitcoind:{}", input.ports.bitcoind_rpc),
        flag!("--daemon-p2p-addr=bitcoind:{}", input.ports.bitcoind_p2p),
        flag!("--electrum-rpc-addr=0.0.0.0:{}", input.ports.electrs),
        flag!("--log-filters=INFO"),
    ];

    let command_asb_controller = command![
        "asb-controller",
        flag!("--url=http://asb:{}", input.ports.asb_rpc_port),
    ];

    let date = chrono::Utc::now()
        .format("%Y-%m-%d %H:%M:%S UTC")
        .to_string();

    let compose_str = format!(
        "\
# This file was auto-generated by `orchestrator` on {date}
#
# It is pinned to build the `asb` and `asb-controller` images from this commit:
# {PINNED_GIT_REPOSITORY}
#
# If the code does not match the hash, the build will fail. This ensures that the code cannot be altered by Github.
# The compiled `orchestrator` has this hash burned into the binary.
#
# To update the `asb` and `asb-controller` images, you need to either:
# - re-compile the `orchestrator` binary from a commit from Github
# - download a newer pre-compiled version of the `orchestrator` binary from Github.
#
# After updating the `orchestrator` binary, re-generate the compose file by running `orchestrator` again.
#
# The used images for `bitcoind`, `monerod`, `electrs` are pinned to specific hashes which prevents them from being altered by the Docker registry.
#
# Please check for new releases regularly. Breaking network changes are rare, but they do happen from time to time.
name: {project_name}
services:
  monerod:
    container_name: monerod
    {image_monerod}
    restart: unless-stopped
    user: root
    volumes:
      - 'monerod-data:/monerod-data/'
    expose:
      - {port_monerod_rpc}
    entrypoint: ''
    command: {command_monerod}
  bitcoind:
    container_name: bitcoind
    {image_bitcoind}
    restart: unless-stopped
    volumes:
      - 'bitcoind-data:/bitcoind-data/'
    expose:
      - {port_bitcoind_rpc}
      - {port_bitcoind_p2p}
    user: root
    entrypoint: ''
    command: {command_bitcoind}
  electrs:
    container_name: electrs
    {image_electrs}
    restart: unless-stopped
    user: root
    depends_on:
      - bitcoind
    volumes:
      - 'bitcoind-data:/bitcoind-data'
      - 'electrs-data:/electrs-data'
    expose:
      - {electrs_port}
    entrypoint: ''
    command: {command_electrs}
  asb:
    container_name: asb
    {image_asb}
    restart: unless-stopped
    depends_on:
      - electrs
    volumes:
      - '{asb_config_path_on_host}:{asb_config_path_inside_container}'
      - 'asb-data:{asb_data_dir}'
    ports:
      - '0.0.0.0:{asb_port}:{asb_port}'
    entrypoint: ''
    command: {command_asb}
  asb-controller:
    container_name: asb-controller
    {image_asb_controller}
    stdin_open: true
    tty: true
    restart: unless-stopped
    depends_on:
      - asb
    entrypoint: ''
    command: {command_asb_controller}
volumes:
  monerod-data:
  bitcoind-data:
  electrs-data:
  asb-data:
",
        port_monerod_rpc = input.ports.monerod_rpc,
        port_bitcoind_rpc = input.ports.bitcoind_rpc,
        port_bitcoind_p2p = input.ports.bitcoind_p2p,
        electrs_port = input.ports.electrs,
        asb_port = input.ports.asb_libp2p,
        image_monerod = input.images.monerod.to_image_attribute(),
        image_electrs = input.images.electrs.to_image_attribute(),
        image_bitcoind = input.images.bitcoind.to_image_attribute(),
        image_asb = input.images.asb.to_image_attribute(),
        image_asb_controller = input.images.asb_controller.to_image_attribute(),
        asb_data_dir = input.directories.asb_data_dir.display(),
        asb_config_path_on_host = input.directories.asb_config_path_on_host(),
        asb_config_path_inside_container = input.directories.asb_config_path_inside_container().display(),
    );

    validate_compose(&compose_str);

    compose_str
}

pub struct Flags(Vec<Flag>);

/// Displays a list of flags into the "Exec form" supported by Docker
/// This is documented here:
/// https://docs.docker.com/reference/dockerfile/#exec-form
///
/// E.g ["/bin/bash", "-c", "echo hello"]
impl Display for Flags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Collect all non-none flags
        let flags = self
            .0
            .iter()
            .filter_map(|f| f.0.as_ref())
            .collect::<Vec<_>>();

        // Put the " around each flag, join with a comma, put the whole thing in []
        write!(
            f,
            "[{}]",
            flags
                .into_iter()
                .map(|f| format!("\"{}\"", f))
                .collect::<Vec<_>>()
                .join(",")
        )
    }
}

pub struct Flag(pub Option<String>);

impl Display for Flag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(s) = &self.0 {
            return write!(f, "{}", s);
        }

        Ok(())
    }
}

pub trait IntoFlag {
    /// Converts into a flag that can be used in a docker compose file
    fn to_flag(self) -> Flag;
    /// Converts into a string that can be used for display purposes
    fn to_display(self) -> &'static str;
}

impl IntoFlag for monero::Network {
    /// This is documented here:
    /// https://docs.getmonero.org/interacting/monerod-reference/#pick-monero-network-blockchain
    fn to_flag(self) -> Flag {
        Flag(match self {
            monero::Network::Mainnet => None,
            monero::Network::Stagenet => Some("--stagenet".to_string()),
            monero::Network::Testnet => Some("--testnet".to_string()),
        })
    }

    fn to_display(self) -> &'static str {
        match self {
            monero::Network::Mainnet => "mainnet",
            monero::Network::Stagenet => "stagenet",
            monero::Network::Testnet => "testnet",
        }
    }
}

impl IntoFlag for bitcoin::Network {
    /// This is documented here:
    /// https://www.mankier.com/1/bitcoind
    fn to_flag(self) -> Flag {
        Flag(Some(match self {
            bitcoin::Network::Bitcoin => "-chain=main".to_string(),
            bitcoin::Network::Testnet => "-chain=test".to_string(),
            _ => panic!("Only Mainnet and Testnet are supported"),
        }))
    }

    fn to_display(self) -> &'static str {
        match self {
            bitcoin::Network::Bitcoin => "mainnet",
            bitcoin::Network::Testnet => "testnet",
            _ => panic!("Only Mainnet and Testnet are supported"),
        }
    }
}

pub trait IntoSpec {
    fn to_spec(self) -> String;
}

impl IntoSpec for OrchestratorInput {
    fn to_spec(self) -> String {
        build(self)
    }
}

/// Converts something into either a:
/// - image: <image>
/// - build: <url to git repo>
pub trait IntoImageAttribute {
    fn to_image_attribute(self) -> String;
}

impl IntoImageAttribute for OrchestratorImage {
    fn to_image_attribute(self) -> String {
        match self {
            OrchestratorImage::Registry(image) => format!("image: {}", image),
            OrchestratorImage::Build(input) => format!(
                r#"build: {{ context: "{}", dockerfile: "{}" }}"#,
                input.context, input.dockerfile
            ),
        }
    }
}

fn validate_compose(compose_str: &str) {
    serde_yaml::from_str::<Compose>(compose_str).expect(&format!(
        "Generated compose spec to be valid. But it was not. This is the spec: \n\n{}",
        compose_str
    ));
}
