use swap_orchestrator::compose::{
    IntoSpec, OrchestratorDirectories, OrchestratorImage, OrchestratorImages, OrchestratorInput,
    OrchestratorNetworks, OrchestratorPorts,
};
use swap_orchestrator::images;

#[test]
fn test_orchestrator_spec_generation() {
    let input = OrchestratorInput {
        ports: OrchestratorPorts {
            monerod_rpc: 38081,
            bitcoind_rpc: 18332,
            bitcoind_p2p: 18333,
            electrs: 60001,
            tor_socks: 9050,
            asb_libp2p: 9839,
            asb_rpc_port: 9944,
            rendezvous_node_port: 8888,
        },
        networks: OrchestratorNetworks {
            monero: monero::Network::Stagenet,
            bitcoin: bitcoin::Network::Testnet,
        },
        images: OrchestratorImages {
            monerod: OrchestratorImage::Registry(images::MONEROD_IMAGE.to_string()),
            electrs: OrchestratorImage::Registry(images::ELECTRS_IMAGE.to_string()),
            bitcoind: OrchestratorImage::Registry(images::BITCOIND_IMAGE.to_string()),
            tor: OrchestratorImage::Registry(images::TOR_IMAGE.to_string()),
            rendezvous_node: OrchestratorImage::Build(
                images::RENDEZVOUS_NODE_IMAGE_FROM_SOURCE.clone(),
            ),
            asb: OrchestratorImage::Build(images::ASB_IMAGE_FROM_SOURCE.clone()),
            asb_controller: OrchestratorImage::Build(
                images::ASB_CONTROLLER_IMAGE_FROM_SOURCE.clone(),
            ),
            asb_tracing_logger: OrchestratorImage::Registry(
                images::ASB_TRACING_LOGGER_IMAGE.to_string(),
            ),
        },
        directories: OrchestratorDirectories {
            asb_data_dir: std::path::PathBuf::from(swap_orchestrator::compose::ASB_DATA_DIR),
        },
        want_tor: false,
    };

    let spec = input.to_spec();

    println!("{}", spec);

    // TODO: Here we should use the docker binary to verify the compose file
}
