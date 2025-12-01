use crate::containers;
use crate::containers::*;
use crate::images::PINNED_GIT_REPOSITORY;
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
    pub want_tor: bool,
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
    pub tor: T,
    pub asb: T,
    pub asb_controller: T,
    pub asb_tracing_logger: T,
    pub rendezvous_node: T,
}

pub struct OrchestratorPorts {
    pub monerod_rpc: u16,
    pub bitcoind_rpc: u16,
    pub bitcoind_p2p: u16,
    pub electrs: u16,
    pub tor_socks: u16,
    pub asb_libp2p: u16,
    pub asb_rpc_port: u16,
    pub rendezvous_node_port: u16,
}

impl From<OrchestratorNetworks<monero::Network, bitcoin::Network>> for OrchestratorPorts {
    fn from(val: OrchestratorNetworks<monero::Network, bitcoin::Network>) -> Self {
        match (val.monero, val.bitcoin) {
            (monero::Network::Mainnet, bitcoin::Network::Bitcoin) => OrchestratorPorts {
                monerod_rpc: 18081,
                bitcoind_rpc: 8332,
                bitcoind_p2p: 8333,
                electrs: 50001,
                tor_socks: 9050,
                asb_libp2p: 9939,
                asb_rpc_port: 9944,
                rendezvous_node_port: 8888,
            },
            (monero::Network::Stagenet, bitcoin::Network::Testnet) => OrchestratorPorts {
                monerod_rpc: 38081,
                bitcoind_rpc: 18332,
                bitcoind_p2p: 18333,
                electrs: 50001,
                tor_socks: 9050,
                asb_libp2p: 9839,
                asb_rpc_port: 9944,
                rendezvous_node_port: 8888,
            },
            _ => panic!("Unsupported Bitcoin / Monero network combination"),
        }
    }
}

impl From<OrchestratorNetworks<monero::Network, bitcoin::Network>> for asb::Network {
    fn from(val: OrchestratorNetworks<monero::Network, bitcoin::Network>) -> Self {
        containers::asb::Network::new(val.monero, val.bitcoin)
    }
}

impl From<OrchestratorNetworks<monero::Network, bitcoin::Network>> for electrs::Network {
    fn from(val: OrchestratorNetworks<monero::Network, bitcoin::Network>) -> Self {
        containers::electrs::Network::new(val.bitcoin)
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

    pub fn asb_config_path_on_host_as_path_buf(&self) -> PathBuf {
        PathBuf::from(self.asb_config_path_on_host())
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
    ($want:expr; $flag:expr, $($args:expr),*) => {
        Flag(if $want { Some(format!($flag, $($args),*)) } else { None })
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

    // monerod's --proxy addr:port and --tx-proxy tor,addr;port can only take numeric addr,
    // and fail with "Exception in main! Failed to initialize p2p server." if given a hostname,
    // so we must resolve the name ourselves. Userland is busybox.
    let command_monerod = command![
        "sh",
        flag!("-xc"),
        flag!(
            r#"
        if {:?}; then
            tor="$(nslookup tor | awk '/answer/,0 {{ if(/Address/) {{ print $2; exit }} }}')"
            set -- "$@" "--proxy=$tor:{}"
        fi
        exec "$@""#,
            input.want_tor,
            input.ports.tor_socks
        ),
        flag!(""),
        flag!("monerod"),
        input.networks.monero.to_flag(),
        flag!("--rpc-bind-ip=0.0.0.0"),
        flag!("--rpc-bind-port={}", input.ports.monerod_rpc),
        flag!("--data-dir=/monerod-data/"),
        flag!("--confirm-external-bind"),
        flag!("--restricted-rpc"),
        flag!("--non-interactive"),
        flag!("--enable-dns-blocklist"),
        // flag!(input.want_tor; "--proxy=tor:{}", input.ports.tor_socks), // the shell program above does the equivalent of this
    ];

    let command_bitcoind = command![
        "bitcoind",
        input.networks.bitcoin.to_flag(),
        flag!("-rpcallowip=0.0.0.0/0"),
        flag!("-rpcbind=0.0.0.0:{}", input.ports.bitcoind_rpc),
        flag!("-bind=0.0.0.0:{}", input.ports.bitcoind_p2p),
        flag!("-datadir=/bitcoind-data/"),
        flag!(input.want_tor; "-proxy=tor:{}", input.ports.tor_socks),
        flag!("-dbcache=16384"),
        // These are required for electrs
        // See: See: https://github.com/romanz/electrs/blob/master/doc/config.md#bitcoind-configuration
        flag!("-server=1"),
        flag!("-prune=0"),
        flag!("-txindex=1"),
    ];

    let electrs_network: containers::electrs::Network = input.networks.clone().into();

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

    let command_asb_tracing_logger = command![
        "sh",
        flag!("-c"),
        flag!("exec tail -f /asb-data/logs/tracing*.log"),
    ];

    let command_rendezvous_node = command![
        "rendezvous-node",
        flag!("--data-dir=/rendezvous-data"),
        flag!("--port={}", input.ports.rendezvous_node_port),
    ];

    let date = chrono::Utc::now()
        .format("%Y-%m-%d %H:%M:%S UTC")
        .to_string();

    let (tor_segment, tor_volume) = if input.want_tor {
        // This image comes with an empty /etc/tor/, so this is the entire config
        let command_tor = command![
            "tor",
            flag!("SocksPort"),
            flag!("0.0.0.0:{}", input.ports.tor_socks),
            flag!("DataDirectory"),
            flag!("/var/lib/tor"),
        ];

        let tor_segment = format!(
            "\
  tor:
    container_name: tor
    {image_tor}
    restart: unless-stopped
    volumes:
      - 'tor-data:/var/lib/tor/'
    expose:
      - {port_tor_socks}
    entrypoint: ''
    command: {command_tor}\
",
            port_tor_socks = input.ports.tor_socks,
            image_tor = input.images.tor.to_image_attribute(),
        );
        (tor_segment, "tor-data:")
    } else {
        (String::new(), "")
    };
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
# The used images for `bitcoind`, `monerod`, `electrs`, `tor` are pinned to specific hashes which prevents them from being altered by the Docker registry.
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
  {tor_segment}
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
  asb-tracing-logger:
    container_name: asb-tracing-logger
    {image_asb_tracing_logger}
    restart: unless-stopped
    depends_on:
      - asb
    volumes:
      - 'asb-data:/asb-data:ro'
    entrypoint: ''
    command: {command_asb_tracing_logger}
  rendezvous-node:
    container_name: rendezvous-node
    {image_rendezvous_node}
    restart: unless-stopped
    volumes:
      - 'rendezvous-data:/rendezvous-data'
    ports:
      - '0.0.0.0:{rendezvous_node_port}:{rendezvous_node_port}'
    entrypoint: ''
    command: {command_rendezvous_node}
volumes:
  monerod-data:
  bitcoind-data:
  electrs-data:
  asb-data:
  rendezvous-data:
  {tor_volume}
",
        port_monerod_rpc = input.ports.monerod_rpc,
        port_bitcoind_rpc = input.ports.bitcoind_rpc,
        port_bitcoind_p2p = input.ports.bitcoind_p2p,
        electrs_port = input.ports.electrs,
        asb_port = input.ports.asb_libp2p,
        rendezvous_node_port = input.ports.rendezvous_node_port,
        image_monerod = input.images.monerod.to_image_attribute(),
        image_electrs = input.images.electrs.to_image_attribute(),
        image_bitcoind = input.images.bitcoind.to_image_attribute(),
        image_asb = input.images.asb.to_image_attribute(),
        image_asb_controller = input.images.asb_controller.to_image_attribute(),
        image_asb_tracing_logger = input.images.asb_tracing_logger.to_image_attribute(),
        image_rendezvous_node = input.images.rendezvous_node.to_image_attribute(),
        command_rendezvous_node = command_rendezvous_node,
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

        // String-escape each flag (""s, newline -> \n), join with a comma, put the whole thing in [], escape $ (which is a docker variable)
        write!(
            f,
            "[{}]",
            flags
                .into_iter()
                .map(|f| format!("{:?}", f.replace('$', "$$")))
                .collect::<Vec<_>>()
                .join(",")
        )
    }
}

pub struct Flag(pub Option<String>);

pub trait IntoFlag {
    /// Converts into a flag that can be used in a docker compose file
    fn to_flag(self) -> Flag;
    /// Converts into a string that can be used for display purposes
    fn to_display(self) -> &'static str;
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
            OrchestratorImage::Registry(image) => format!("image: {image}"),
            OrchestratorImage::Build(input) => format!(
                r#"build: {{ context: "{}", dockerfile: "{}" }}"#,
                input.context, input.dockerfile
            ),
        }
    }
}

fn validate_compose(compose_str: &str) {
    serde_yaml::from_str::<Compose>(compose_str).unwrap_or_else(|_| {
        panic!("Expected generated compose spec to be valid. But it was not. This is the spec: \n\n{compose_str}")
    });
}
