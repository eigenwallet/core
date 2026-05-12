#![allow(unused_crate_dependencies)]

use swap_orchestrator::compose::{
    CloudflaredConfig, IntoSpec, OrchestratorDirectories, OrchestratorImage, OrchestratorImages,
    OrchestratorInput, OrchestratorNetworks, OrchestratorPorts,
};
use swap_orchestrator::images;

fn make_input(want_tor: bool, cloudflared: Option<CloudflaredConfig>) -> OrchestratorInput {
    OrchestratorInput {
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
            monero: monero_address::Network::Stagenet,
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
            cloudflared: OrchestratorImage::Registry(images::CLOUDFLARED_IMAGE.to_string()),
        },
        directories: OrchestratorDirectories {
            asb_data_dir: std::path::PathBuf::from(swap_orchestrator::compose::ASB_DATA_DIR),
        },
        want_tor,
        cloudflared,
    }
}

fn sample_cloudflared_config() -> CloudflaredConfig {
    CloudflaredConfig {
        token: "test-token".to_string(),
        external_host: "atomic.exolix.com".to_string(),
        external_port: 443,
        internal_port: 8080,
    }
}

#[test]
fn test_orchestrator_spec_generation() {
    // `to_spec` runs `validate_compose` internally, so generating each
    // variant is enough to catch indentation regressions in the optional
    // tor / cloudflared segments.
    let _ = make_input(false, None).to_spec();
    let _ = make_input(true, None).to_spec();
    let _ = make_input(false, Some(sample_cloudflared_config())).to_spec();
    let _ = make_input(true, Some(sample_cloudflared_config())).to_spec();
}
