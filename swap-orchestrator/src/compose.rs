use crate::containers;
use crate::containers::*;
use crate::images::PINNED_GIT_REPOSITORY;
use compose_spec::Compose;
use std::{
    fmt::{self, Display},
    path::PathBuf,
};

/// Per-container docker `json-file` log rotation, referenced by every service.
/// `max-file * max-size` is the hard cap on a container's daemon logs before the
/// oldest file is dropped (5 * 1g = 5GB).
pub const DOCKER_LOG_MAX_SIZE: &str = "1g";
pub const DOCKER_LOG_MAX_FILE: &str = "5";

pub const ASB_DATA_DIR: &str = "/asb-data";
pub const ASB_CONFIG_FILE: &str = "config.toml";
pub const ASB_RPC_AUTH_FILE_ON_HOST: &str = "./rpc-auth";
pub const ASB_RPC_AUTH_FILE_IN_CONTAINER: &str = "/rpc-auth";
pub const DOCKER_COMPOSE_FILE: &str = "./docker-compose.yml";
pub const PROMTAIL_CONFIG_FILE: &str = "./promtail.yml";
pub const PROMETHEUS_CONFIG_FILE: &str = "./prometheus.yml";

/// Port `cloudflared` serves its built-in Prometheus metrics on, scraped by the
/// prometheus-agent over the docker network.
pub const CLOUDFLARED_METRICS_PORT: u16 = 2000;

pub struct OrchestratorInput {
    pub ports: OrchestratorPorts,
    pub networks: OrchestratorNetworks<monero_address::Network, bitcoin::Network>,
    pub images: OrchestratorImages<OrchestratorImage>,
    pub directories: OrchestratorDirectories,
    pub want_tor: bool,
    pub cloudflared: Option<CloudflaredConfig>,
    pub promtail: Option<PromtailConfig>,
    pub metrics: Option<MetricsConfig>,
    pub gluetun: Option<GluetunConfig>,
}

/// WireGuard VPN (gluetun) configuration.
///
/// When set, the orchestrator adds a `gluetun` service to the compose file
/// and runs the ASB inside its network namespace, so all ASB traffic leaves
/// through the WireGuard tunnel (with gluetun's firewall as kill switch).
/// The other containers keep their normal networking.
///
/// Gluetun is pointed at Docker's embedded DNS so the ASB can resolve the
/// `monerod` / `electrs` service names from inside gluetun's network namespace;
/// gluetun's `asb` network alias lets the other containers dial the ASB by
/// hostname even though it has no network identity of its own.
///
/// If the chosen provider does not support port forwarding, the ASB is not
/// reachable over clearnet TCP — inbound connections require a Tor hidden
/// service or a Cloudflare Tunnel.
#[derive(Clone)]
pub struct GluetunConfig {
    /// Gluetun VPN service provider name (see
    /// <https://github.com/qdm12/gluetun-wiki>).
    pub vpn_service_provider: String,
    /// WireGuard private key from the provider's WireGuard configuration.
    pub wireguard_private_key: String,
    /// WireGuard interface address from the same configuration.
    pub wireguard_addresses: String,
}

/// The compose network uses a fixed subnet so gluetun's outbound firewall can
/// allow the docker network explicitly (`FIREWALL_OUTBOUND_SUBNETS`) — traffic
/// to the other containers must bypass the VPN kill switch.
pub const DOCKER_SUBNET: &str = "172.28.0.0/24";

/// Cloudflare Tunnel configuration.
///
/// When set, the orchestrator adds a `cloudflared` service to the compose file
/// and configures the ASB to listen on a WebSocket transport and advertise the
/// tunnel's public hostname as an external libp2p address.
#[derive(Clone)]
pub struct CloudflaredConfig {
    /// The tunnel run token from the Cloudflare Zero Trust dashboard.
    pub token: String,
    /// The public hostname assigned to the tunnel in the Cloudflare dashboard
    /// (e.g. `asb.example.com`). Advertised to peers as `/dns4/<host>/tcp/<port>/wss`.
    pub external_host: String,
    /// The port clients will dial on the public hostname.
    /// Almost always `443` for `wss`.
    pub external_port: u16,
    /// The port the ASB will listen on inside the docker network for the
    /// WebSocket transport. The tunnel's ingress rule should point at
    /// `http://asb:<internal_port>`.
    pub internal_port: u16,
}

/// Promtail log-shipper configuration.
///
/// When set, the orchestrator adds `promtail` and `docker-socket-proxy`
/// services to the compose file and writes a `promtail.yml` next to
/// `docker-compose.yml`. The shipper tails the JSON tracing logs from the
/// `asb-data` volume (mounted read-only) and the stdout of the
/// `bitcoind`/`monerod`/`electrs` containers (read via the socket proxy),
/// then pushes everything to a Loki endpoint over HTTPS with a bearer token.
#[derive(Clone)]
pub struct PromtailConfig {
    /// Loki push endpoint, e.g.
    /// `https://loki-asb-logs.example.com/loki/api/v1/push`.
    pub loki_push_url: String,
    /// Bearer token presented to the Loki endpoint. Baked into the generated
    /// `promtail.yml` only — never written to `docker-compose.yml`.
    pub loki_push_token: String,
    /// Short identifier for this host (e.g. `asb-de-1`). Exported as the
    /// `host` Loki label on both the asb and node log streams so operators
    /// can filter a whole deployment in Grafana.
    pub instance: String,
}

#[derive(Clone)]
pub struct MetricsConfig {
    pub remote_write_url: String,
    pub token: String,
    pub instance: String,
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
    pub cloudflared: T,
    pub promtail: T,
    pub docker_socket_proxy: T,
    pub cadvisor: T,
    pub prometheus_agent: T,
    pub gluetun: T,
}

pub struct OrchestratorPorts {
    pub monerod_rpc: u16,
    pub bitcoind_rpc: u16,
    pub bitcoind_p2p: u16,
    pub electrs: u16,
    pub tor_socks: u16,
    pub asb_libp2p: u16,
    pub asb_rpc_port: u16,
    pub asb_metrics_port: u16,
    pub rendezvous_node_port: u16,
}

impl From<OrchestratorNetworks<monero_address::Network, bitcoin::Network>> for OrchestratorPorts {
    fn from(val: OrchestratorNetworks<monero_address::Network, bitcoin::Network>) -> Self {
        match (val.monero, val.bitcoin) {
            (monero_address::Network::Mainnet, bitcoin::Network::Bitcoin) => OrchestratorPorts {
                monerod_rpc: 18081,
                bitcoind_rpc: 8332,
                bitcoind_p2p: 8333,
                electrs: 50001,
                tor_socks: 9050,
                asb_libp2p: 9939,
                asb_rpc_port: 9944,
                asb_metrics_port: 9945,
                rendezvous_node_port: 8888,
            },
            (monero_address::Network::Stagenet, bitcoin::Network::Testnet) => OrchestratorPorts {
                monerod_rpc: 38081,
                bitcoind_rpc: 18332,
                bitcoind_p2p: 18333,
                electrs: 50001,
                tor_socks: 9050,
                asb_libp2p: 9839,
                asb_rpc_port: 9944,
                asb_metrics_port: 9945,
                rendezvous_node_port: 8888,
            },
            _ => panic!("Unsupported Bitcoin / Monero network combination"),
        }
    }
}

impl From<OrchestratorNetworks<monero_address::Network, bitcoin::Network>> for asb::Network {
    fn from(val: OrchestratorNetworks<monero_address::Network, bitcoin::Network>) -> Self {
        containers::asb::Network::new(val.monero, val.bitcoin)
    }
}

impl From<OrchestratorNetworks<monero_address::Network, bitcoin::Network>> for electrs::Network {
    fn from(val: OrchestratorNetworks<monero_address::Network, bitcoin::Network>) -> Self {
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
    // Root of the Cargo workspace; may embed a token in the URL userinfo for a private repo.
    pub context: String,
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
        flag!("--rpc-auth-file={}", ASB_RPC_AUTH_FILE_IN_CONTAINER),
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

    let cloudflared_segment = if let Some(cf) = input.cloudflared.as_ref() {
        // We clear the image's ENTRYPOINT below, so `command` must start with
        // the binary name, matching every other service in this compose file.
        let command_cloudflared = command![
            "cloudflared",
            flag!("--no-autoupdate"),
            flag!("tunnel"),
            flag!("--metrics"),
            flag!("0.0.0.0:{}", CLOUDFLARED_METRICS_PORT),
            flag!("run"),
            flag!("--token"),
            flag!("{}", cf.token),
        ];

        format!(
            "\
  cloudflared:
    container_name: cloudflared
    {image_cloudflared}
    restart: unless-stopped
    logging: *default-logging
    depends_on:
      - asb
    expose:
      - {port_cloudflared_metrics}
    entrypoint: ''
    command: {command_cloudflared}\
",
            image_cloudflared = input.images.cloudflared.to_image_attribute(),
            port_cloudflared_metrics = CLOUDFLARED_METRICS_PORT,
        )
    } else {
        String::new()
    };

    let (promtail_segment, promtail_volume) = if input.promtail.is_some() {
        // The promtail config file lives next to docker-compose.yml on the
        // host. It is generated by the orchestrator at the same time as the
        // compose file, with the URL/token/instance values baked in.
        //
        // docker-socket-proxy is the only container that mounts the docker
        // socket. It exposes only the read-only container + network APIs
        // (CONTAINERS=1, NETWORKS=1; POST stays disabled). promtail's docker
        // service discovery needs /networks to compute the network labels in
        // addition to listing containers, so both are required - but it still
        // never holds write/root-equivalent access to the host.
        let promtail_segment = format!(
            "\
  docker-socket-proxy:
    container_name: docker-socket-proxy
    {image_docker_socket_proxy}
    restart: unless-stopped
    logging: *default-logging
    environment:
      - CONTAINERS=1
      - NETWORKS=1
    volumes:
      - '/var/run/docker.sock:/var/run/docker.sock:ro'
    expose:
      - 2375
  promtail:
    container_name: promtail
    {image_promtail}
    restart: unless-stopped
    logging: *default-logging
    depends_on:
      - asb
      - docker-socket-proxy
    volumes:
      - '{promtail_config_file}:/etc/promtail/promtail.yml:ro'
      - 'asb-data:/asb-data:ro'
      - 'promtail-positions:/var/lib/promtail'
    command: [\"-config.file=/etc/promtail/promtail.yml\"]\
",
            image_docker_socket_proxy = input.images.docker_socket_proxy.to_image_attribute(),
            image_promtail = input.images.promtail.to_image_attribute(),
            promtail_config_file = PROMTAIL_CONFIG_FILE,
        );
        (promtail_segment, "promtail-positions:")
    } else {
        (String::new(), "")
    };

    let (metrics_segment, metrics_volume) = if input.metrics.is_some() {
        let metrics_segment = format!(
            "\
  cadvisor:
    container_name: cadvisor
    {image_cadvisor}
    restart: unless-stopped
    logging: *default-logging
    privileged: true
    cgroup: host
    command:
      # Workaround for cadvisor#3860
      - '--disable_metrics=disk'
    devices:
      - /dev/kmsg:/dev/kmsg
    volumes:
      - '/:/rootfs:ro'
      - '/var/run:/var/run:ro'
      - '/sys:/sys:ro'
      - '/var/lib/docker/:/var/lib/docker:ro'
      - '/dev/disk/:/dev/disk:ro'
    expose:
      - 8080
  prometheus-agent:
    container_name: prometheus-agent
    {image_prometheus_agent}
    restart: unless-stopped
    logging: *default-logging
    depends_on:
      - cadvisor
    volumes:
      - '{prometheus_config_file}:/etc/prometheus/prometheus.yml:ro'
      - 'prometheus-agent-data:/prometheus'
    command: [\"--config.file=/etc/prometheus/prometheus.yml\", \"--agent\", \"--storage.agent.path=/prometheus\"]\
",
            image_cadvisor = input.images.cadvisor.to_image_attribute(),
            image_prometheus_agent = input.images.prometheus_agent.to_image_attribute(),
            prometheus_config_file = PROMETHEUS_CONFIG_FILE,
        );
        (metrics_segment, "prometheus-agent-data:")
    } else {
        (String::new(), "")
    };

    let gluetun_segment = if let Some(gluetun) = input.gluetun.as_ref() {
        // Everything that dials the ASB enters the shared namespace through
        // gluetun's docker interface, so its firewall must accept those
        // ports explicitly.
        let mut input_ports = vec![input.ports.asb_libp2p, input.ports.asb_rpc_port];
        if input.metrics.is_some() {
            input_ports.push(input.ports.asb_metrics_port);
        }
        if let Some(cf) = input.cloudflared.as_ref() {
            input_ports.push(cf.internal_port);
        }
        let firewall_input_ports = input_ports
            .iter()
            .map(u16::to_string)
            .collect::<Vec<_>>()
            .join(",");

        format!(
            "\
  gluetun:
    container_name: gluetun
    {image_gluetun}
    restart: unless-stopped
    logging: *default-logging
    networks:
      default:
        aliases:
          - asb
    cap_add:
      - NET_ADMIN
    devices:
      - /dev/net/tun:/dev/net/tun
    sysctls:
      - net.ipv4.tcp_tw_reuse=1
    environment:
      VPN_SERVICE_PROVIDER: {vpn_service_provider}
      VPN_TYPE: wireguard
      WIREGUARD_PRIVATE_KEY: {wireguard_private_key}
      WIREGUARD_ADDRESSES: {wireguard_addresses}
      FIREWALL_OUTBOUND_SUBNETS: {subnet}
      FIREWALL_INPUT_PORTS: '{firewall_input_ports}'
      DNS_ADDRESS: 127.0.0.11
    ports:
      - '0.0.0.0:{asb_port}:{asb_port}'\
",
            image_gluetun = input.images.gluetun.to_image_attribute(),
            vpn_service_provider = yaml_compose_value(&gluetun.vpn_service_provider),
            wireguard_private_key = yaml_compose_value(&gluetun.wireguard_private_key),
            wireguard_addresses = yaml_compose_value(&gluetun.wireguard_addresses),
            subnet = DOCKER_SUBNET,
            asb_port = input.ports.asb_libp2p,
        )
    } else {
        String::new()
    };

    // gluetun owns the shared network namespace: the net sysctl must sit on it
    // (Docker rejects net sysctls on a namespace-sharing container) and the ASB
    // waits for the tunnel to be healthy so it cannot leak traffic outside it.
    let asb_sysctls_and_depends_on = if input.gluetun.is_some() {
        "depends_on:\n      electrs:\n        condition: service_started\n      gluetun:\n        condition: service_healthy"
    } else {
        "sysctls:\n      - net.ipv4.tcp_tw_reuse=1\n    depends_on:\n      - electrs"
    };

    let networks_segment = if input.gluetun.is_some() {
        format!(
            "networks:\n  default:\n    ipam:\n      config:\n        - subnet: {DOCKER_SUBNET}\n"
        )
    } else {
        String::new()
    };

    // The ASB shares gluetun's namespace and so cannot publish ports; gluetun
    // publishes the libp2p port for it.
    let asb_network = if input.gluetun.is_some() {
        r#"network_mode: "service:gluetun""#.to_string()
    } else {
        format!(
            "ports:\n      - '0.0.0.0:{port}:{port}'",
            port = input.ports.asb_libp2p
        )
    };

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
    logging: *default-logging
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
x-logging: &default-logging
  driver: json-file
  options:
    max-size: '{log_max_size}'
    max-file: '{log_max_file}'
services:
  monerod:
    container_name: monerod
    {image_monerod}
    restart: unless-stopped
    logging: *default-logging
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
    logging: *default-logging
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
    logging: *default-logging
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
  {cloudflared_segment}
  {promtail_segment}
  {metrics_segment}
  {gluetun_segment}
  asb:
    container_name: asb
    {image_asb}
    restart: unless-stopped
    logging: *default-logging
    cap_add:
      - SYS_PTRACE
    {asb_sysctls_and_depends_on}
    volumes:
      - '{asb_config_path_on_host}:{asb_config_path_inside_container}'
      # makes `docker compose up` fail if the keyfile is missing
      - type: bind
        source: '{asb_rpc_auth_file_on_host}'
        target: '{asb_rpc_auth_file_in_container}'
        read_only: true
        bind:
          create_host_path: false
      - 'asb-data:{asb_data_dir}'
    {asb_network}
    entrypoint: ''
    command: {command_asb}
  asb-controller:
    container_name: asb-controller
    {image_asb_controller}
    stdin_open: true
    tty: true
    restart: unless-stopped
    logging: *default-logging
    depends_on:
      - asb
    entrypoint: ''
    command: {command_asb_controller}
  asb-tracing-logger:
    container_name: asb-tracing-logger
    {image_asb_tracing_logger}
    restart: unless-stopped
    logging: *default-logging
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
    logging: *default-logging
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
  {promtail_volume}
  {metrics_volume}
{networks_segment}",
        log_max_size = DOCKER_LOG_MAX_SIZE,
        log_max_file = DOCKER_LOG_MAX_FILE,
        port_monerod_rpc = input.ports.monerod_rpc,
        port_bitcoind_rpc = input.ports.bitcoind_rpc,
        port_bitcoind_p2p = input.ports.bitcoind_p2p,
        electrs_port = input.ports.electrs,
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
        asb_rpc_auth_file_on_host = ASB_RPC_AUTH_FILE_ON_HOST,
        asb_rpc_auth_file_in_container = ASB_RPC_AUTH_FILE_IN_CONTAINER,
    );

    validate_compose(&compose_str);

    compose_str
}

/// Builds the YAML body of `promtail.yml`.
///
/// Values from [`PromtailConfig`] are baked directly into the file — the
/// container does not need env-var expansion at runtime, and the bearer
/// token never appears in `docker-compose.yml`.
///
/// Two scrape jobs are emitted, both labelled with the same `host` so a
/// deployment can be selected as a whole:
/// - `asb-tracing` tails every `*.log` file under `/asb-data/logs/` (where
///   `asb` writes `tracing.*`, `tracing-libp2p.*`, `tracing-monero-wallet.*`,
///   `tracing-tor.*`, etc.) and labels each stream with the component
///   extracted from the file name.
/// - `node` discovers the `bitcoind`/`monerod`/`electrs` containers through
///   the docker-socket-proxy and tails their stdout, labelling each stream
///   with `job: node` and the `container` name. These daemons log plain text
///   (electrs has no log file at all), so they are shipped as raw lines
///   rather than parsed as JSON.
pub fn build_promtail_yml(cfg: &PromtailConfig) -> String {
    // The single quote in YAML single-quoted strings is escaped by doubling
    // it. We single-quote every interpolated value so URLs containing
    // colons/slashes and tokens with special characters stay literal.
    fn yaml_single_quote(value: &str) -> String {
        format!("'{}'", value.replace('\'', "''"))
    }

    format!(
        "\
server:
  http_listen_port: 9080
  grpc_listen_port: 0

positions:
  filename: /var/lib/promtail/positions.yaml

clients:
  - url: {url}
    bearer_token: {token}
    backoff_config:
      min_period: 1s
      max_period: 5m
      max_retries: 0

scrape_configs:
  - job_name: asb-tracing
    static_configs:
      - targets: [localhost]
        labels:
          job: asb
          host: {instance}
          __path__: /asb-data/logs/*.log
    pipeline_stages:
      - regex:
          source: filename
          expression: '/asb-data/logs/(?P<component>[^./]+)'
      - labels:
          component:
      - json:
          expressions:
            level: level
            ts: timestamp
            msg: fields.message
      - timestamp:
          source: ts
          format: RFC3339Nano
          fallback_formats:
            - RFC3339
          action_on_failure: skip
      - labels:
          level:
  - job_name: node
    docker_sd_configs:
      - host: tcp://docker-socket-proxy:2375
        refresh_interval: 5s
    relabel_configs:
      - source_labels: [__meta_docker_container_name]
        regex: '/?(bitcoind|monerod|electrs)'
        action: keep
      - source_labels: [__meta_docker_container_name]
        regex: '/?(.*)'
        target_label: container
        replacement: '$1'
      - target_label: job
        replacement: node
      - target_label: host
        replacement: {instance}
",
        url = yaml_single_quote(&cfg.loki_push_url),
        token = yaml_single_quote(&cfg.loki_push_token),
        instance = yaml_single_quote(&cfg.instance),
    )
}

/// Builds `prometheus.yml`. Always scrapes cadvisor and the ASB's libp2p
/// endpoint; also scrapes cloudflared's metrics endpoint when
/// `scrape_cloudflared` is set.
pub fn build_prometheus_agent_yml(
    cfg: &MetricsConfig,
    asb_metrics_port: u16,
    scrape_cloudflared: bool,
) -> String {
    fn yaml_single_quote(value: &str) -> String {
        format!("'{}'", value.replace('\'', "''"))
    }

    let cloudflared_scrape = if scrape_cloudflared {
        format!(
            "
  - job_name: cloudflared
    static_configs:
      - targets: ['cloudflared:{CLOUDFLARED_METRICS_PORT}']"
        )
    } else {
        String::new()
    };

    format!(
        "\
global:
  scrape_interval: 30s
  external_labels:
    host: {instance}

scrape_configs:
  - job_name: cadvisor
    static_configs:
      - targets: ['cadvisor:8080']
  - job_name: asb
    static_configs:
      - targets: ['asb:{asb_metrics_port}']{cloudflared_scrape}

remote_write:
  - url: {url}
    bearer_token: {token}
",
        instance = yaml_single_quote(&cfg.instance),
        url = yaml_single_quote(&cfg.remote_write_url),
        token = yaml_single_quote(&cfg.token),
    )
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
                r#"build: {{ context: "{}", dockerfile: "{}", network: "host" }}"#,
                input.context, input.dockerfile
            ),
        }
    }
}

/// Single-quotes a value for use in the compose file. `$` is doubled because
/// compose performs variable interpolation even inside single quotes.
fn yaml_compose_value(value: &str) -> String {
    format!("'{}'", value.replace('$', "$$").replace('\'', "''"))
}

fn validate_compose(compose_str: &str) {
    serde_yaml::from_str::<Compose>(compose_str).unwrap_or_else(|_| {
        panic!("Expected generated compose spec to be valid. But it was not. This is the spec: \n\n{compose_str}")
    });
}
