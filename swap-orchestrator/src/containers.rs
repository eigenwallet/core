use std::{path::PathBuf, sync::Arc};

use url::Url;

///! This meta module describes **how to run** containers
///
/// Currently this only includes which flags we need to pass to the binaries
use crate::{
    command,
    compose::{ComposeConfig, Flag, ImageSource, Mount, Service, Volume},
    flag,
    images::{self, PINNED_GIT_REPOSITORY},
};

// Important: don't add slashes or anything here
// Todo: find better way to do that
const MONEROD_DATA: &str = "monerod-data";
const BITCOIN_DATA: &str = "bitcoin-data";
const ELECTRS_DATA: &str = "electrs-data";
const ASB_DATA: &str = "asb-data";

/// Add all the services/volumes to the compose config.
/// Returns urls for the electrum rpc and monero rpc endpoints.
#[allow(unused_variables)]
pub fn add_maker_services(
    compose: &mut ComposeConfig,
    bitcoin_network: bitcoin::Network,
    monero_network: monero::Network,
    create_full_bitcoin_node: bool,
    create_full_monero_node: bool,
) -> (Arc<Volume>, Url, Url) {
    let (monerod_data, monerod, monerod_rpc_port) =
        monerod(compose, monero_network, create_full_monero_node);
    let (bitcoind_data, bitcoind, bitcoind_rpc_port, bitcoind_p2p_port) =
        bitcoind(compose, bitcoin_network, create_full_bitcoin_node);
    let (electrs_data, electrs, electrs_port) = electrs(
        compose,
        bitcoin_network,
        create_full_bitcoin_node,
        bitcoind_rpc_port,
        bitcoind_p2p_port,
        bitcoind,
        bitcoind_data,
    );
    let (asb_data, asb, asb_p2p_port, asb_rpc_port) = asb(
        compose,
        bitcoin_network,
        monero_network,
        electrs.clone(),
        monerod.clone(),
        true,
        PathBuf::from("./config.toml"),
    );
    let asb_controller = asb_controller(compose, asb_rpc_port, asb.clone());
    let asb_tracing_logger = asb_tracing_logger(compose, asb, asb_data.clone());

    let electrum_rpc_url: Url = format!(
        "tcp://{electrs_name}:{electrs_port}",
        electrs_name = electrs.name()
    )
    .parse()
    .expect("valid url");
    let monerod_rpc_url: Url = format!(
        "http://{monerod_name}:{monerod_rpc_port}",
        monerod_name = monerod.name()
    )
    .parse()
    .expect("valid url");

    (asb_data, electrum_rpc_url, monerod_rpc_url)
}

/// Add the servie/volume to the compose config + return them + the rpc port
pub fn monerod(
    compose: &mut ComposeConfig,
    network: monero::Network,
    enabled: bool,
) -> (Arc<Volume>, Arc<Service>, u16) {
    let (network_flag, monerod_rpc_port): (Option<Flag>, u16) = match network {
        monero::Network::Mainnet => (None, 18081),
        monero::Network::Stagenet => (Some(flag!("--stagenet")), 38081),
        _ => unimplemented!(),
    };

    let monerod_data = compose.add_volume(MONEROD_DATA);
    let mut monerod_command = command![
        "monerod",
        flag!("--rpc-bind-ip=0.0.0.0"),
        flag!("--rpc-bind-port={monerod_rpc_port}"),
        flag!("--data-dir={}", monerod_data.as_root_dir().display()),
        flag!("--confirm-external-bind"),
        flag!("--restricted-rpc"),
        flag!("--non-interactive"),
        flag!("--enable-dns-blocklist")
    ];

    if let Some(network_flag) = network_flag {
        monerod_command.add_flag(network_flag);
    }

    let monerod_service =
        Service::new("monerod", ImageSource::from_registry(images::MONEROD_IMAGE))
            .with_enabled(enabled)
            .with_mount(Mount::volume(&monerod_data))
            .with_exposed_port(monerod_rpc_port)
            .with_command(monerod_command);
    let monerod_service = compose.add_service(monerod_service);

    (monerod_data, monerod_service, monerod_rpc_port)
}

/// Adds the volume/service to the compose config and returns them + the rpc bind port and p2p bind port
pub fn bitcoind(
    compose: &mut ComposeConfig,
    network: bitcoin::Network,
    enabled: bool,
) -> (Arc<Volume>, Arc<Service>, u16, u16) {
    let (rpc_port, p2p_port, chain): (u16, u16, &str) = match network {
        bitcoin::Network::Bitcoin => (8332, 8333, "main"),
        bitcoin::Network::Testnet => (18332, 18333, "test"),
        _ => panic!("unsupported bitcoin network"),
    };

    let bitcoind_data = compose.add_volume(BITCOIN_DATA);

    let bitcoind_command = command!(
        "bitcoind",
        flag!("-chain={chain}"),
        flag!("-rpcallowip=0.0.0.0/0"),
        flag!("-rpcbind=0.0.0.0:{rpc_port}"),
        flag!("-bind=0.0.0.0:{p2p_port}"),
        flag!("-datadir={}", bitcoind_data.as_root_dir().display()),
        flag!("-dbcache=16384"),
        flag!("-server=1"),
        flag!("-prune=0"),
        flag!("-txindex=1"),
    );

    let bitcoind = Service::new(
        "bitcoind",
        ImageSource::from_registry(images::BITCOIND_IMAGE),
    )
    .with_mount(Mount::volume(&bitcoind_data))
    .with_exposed_port(rpc_port)
    .with_exposed_port(p2p_port)
    .with_command(bitcoind_command)
    .with_enabled(enabled);

    let bitcoind = compose.add_service(bitcoind);

    (bitcoind_data, bitcoind, rpc_port, p2p_port)
}

pub fn electrs(
    compose: &mut ComposeConfig,
    network: bitcoin::Network,
    enabled: bool,
    bitcoind_rpc_port: u16,
    bitcoind_p2p_port: u16,
    bitcoind: Arc<Service>,
    bitcoind_data: Arc<Volume>,
) -> (Arc<Volume>, Arc<Service>, u16) {
    let (port, chain): (u16, &str) = match network {
        bitcoin::Network::Bitcoin => (50001, "bitcoin"),
        bitcoin::Network::Testnet => (50001, "testnet"),
        _ => panic!("unsupported bitcoin network"),
    };

    let bitcoind_name = bitcoind.name();

    let electrs_data = compose.add_volume(ELECTRS_DATA);
    let command = command!(
        "electrs",
        flag!("--network={chain}"),
        flag!("--daemon-dir={}", bitcoind_data.as_root_dir().display()),
        flag!(
            "--db-dir={}",
            electrs_data.as_root_dir().join("db").display()
        ),
        flag!("--daemon-rpc-addr={bitcoind_name}:{bitcoind_rpc_port}"),
        flag!("--daemon-p2p-addr={bitcoind_name}:{bitcoind_p2p_port}"),
        flag!("--electrum-rpc-addr=0.0.0.0:{port}"),
        flag!("--log-filters=INFO"),
    );
    let service = Service::new("electrs", ImageSource::from_registry(images::ELECTRS_IMAGE))
        .with_dependency(bitcoind.clone())
        .with_exposed_port(port)
        .with_command(command)
        .with_mount(Mount::volume(&electrs_data))
        .with_enabled(enabled);

    let service = compose.add_service(service);

    (electrs_data, service, port)
}

pub fn asb(
    compose: &mut ComposeConfig,
    bitcoin_network: bitcoin::Network,
    monero_network: monero::Network,
    electrs: Arc<Service>,
    monerod: Arc<Service>,
    build_from_source: bool,
    config_path: PathBuf,
) -> (Arc<Volume>, Arc<Service>, u16, u16) {
    let (network_flag, asb_p2p_port, asb_rpc_port): (Option<Flag>, u16, u16) =
        match (bitcoin_network, monero_network) {
            (bitcoin::Network::Bitcoin, monero::Network::Mainnet) => (None, 9939, 9944),
            (bitcoin::Network::Testnet, monero::Network::Stagenet) => {
                (Some(flag!("--testnet")), 9839, 9944)
            }
            _ => unreachable!("invalid network combination"),
        };

    let asb_data = compose.add_volume(ASB_DATA);
    let container_config_path = asb_data.as_root_dir().join("config.toml");
    let mut command = command![
        "asb",
        flag!("--config={}", container_config_path.display()),
        flag!("start"),
        flag!("--rpc-bind-port={asb_rpc_port}"),
        flag!("--rpc-bind-host=0.0.0.0")
    ];

    if let Some(network_flag) = network_flag {
        command.add_flag(network_flag);
    }

    let image_source = match build_from_source {
        // Todo: allow prebuilt image
        _ => ImageSource::from_source(
            PINNED_GIT_REPOSITORY.parse().expect("valid url"),
            "./swap-asb/Dockerfile",
        ),
    };
    let mut asb_service = Service::new("asb", image_source)
        .with_exposed_port(asb_p2p_port)
        .with_mount(Mount::volume(&asb_data))
        .with_mount(Mount::path(config_path, container_config_path))
        .with_command(command);

    if electrs.is_enabled() {
        asb_service = asb_service.with_dependency(electrs);
    }

    if monerod.is_enabled() {
        asb_service = asb_service.with_dependency(monerod)
    }

    let service = compose.add_service(asb_service);

    (asb_data, service, asb_p2p_port, asb_rpc_port)
}

pub fn asb_controller(
    compose: &mut ComposeConfig,
    asb_rpc_port: u16,
    asb: Arc<Service>,
) -> Arc<Service> {
    let command = command!["asb-controller", flag!("--url=http://asb:{asb_rpc_port}")];

    let service = Service::new(
        "asb-controller",
        ImageSource::from_source(
            PINNED_GIT_REPOSITORY.parse().expect("valid url"),
            "./swap-controller/Dockerfile",
        ),
    )
    .with_dependency(asb)
    .with_stdin_open(true)
    .with_tty(true)
    .with_command(command);

    compose.add_service(service)
}

pub fn asb_tracing_logger(
    compose: &mut ComposeConfig,
    asb: Arc<Service>,
    asb_data: Arc<Volume>,
) -> Arc<Service> {
    let command = command![
        "sh",
        flag!("-c"),
        flag!("tail -f /asb-data/logs/tracing*.log")
    ];

    let service = Service::new(
        "asb-tracing-logger",
        ImageSource::from_registry(images::ASB_TRACING_LOGGER_IMAGE),
    )
    .with_dependency(asb)
    .with_mount(Mount::volume(&asb_data))
    .with_command(command);

    compose.add_service(service)
}
