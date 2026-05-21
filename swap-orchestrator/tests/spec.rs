#![allow(unused_crate_dependencies)]

use swap_orchestrator::compose::{
    CloudflaredConfig, IntoSpec, OrchestratorDirectories, OrchestratorImage, OrchestratorImages,
    OrchestratorInput, OrchestratorNetworks, OrchestratorPorts, PromtailConfig,
    build_promtail_yml,
};
use swap_orchestrator::images;

fn make_input(
    want_tor: bool,
    cloudflared: Option<CloudflaredConfig>,
    promtail: Option<PromtailConfig>,
) -> OrchestratorInput {
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
            promtail: OrchestratorImage::Registry(images::PROMTAIL_IMAGE.to_string()),
        },
        directories: OrchestratorDirectories {
            asb_data_dir: std::path::PathBuf::from(swap_orchestrator::compose::ASB_DATA_DIR),
        },
        want_tor,
        cloudflared,
        promtail,
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

fn sample_promtail_config() -> PromtailConfig {
    PromtailConfig {
        loki_push_url: "https://loki-asb-logs.example.com/loki/api/v1/push".to_string(),
        loki_push_token: "test-token".to_string(),
        instance: "asb-test-1".to_string(),
    }
}

#[test]
fn test_orchestrator_spec_generation() {
    // `to_spec` runs `validate_compose` internally, so generating each
    // variant is enough to catch indentation regressions in the optional
    // tor / cloudflared / promtail segments.
    let _ = make_input(false, None, None).to_spec();
    let _ = make_input(true, None, None).to_spec();
    let _ = make_input(false, Some(sample_cloudflared_config()), None).to_spec();
    let _ = make_input(true, Some(sample_cloudflared_config()), None).to_spec();
    let _ = make_input(false, None, Some(sample_promtail_config())).to_spec();
    let _ = make_input(true, None, Some(sample_promtail_config())).to_spec();
    let _ = make_input(
        true,
        Some(sample_cloudflared_config()),
        Some(sample_promtail_config()),
    )
    .to_spec();
}

#[test]
fn test_promtail_yml_is_valid_yaml() {
    let yml = build_promtail_yml(&sample_promtail_config());
    let parsed: serde_yaml::Value =
        serde_yaml::from_str(&yml).expect("promtail.yml must be valid YAML");

    // Sanity-check that the labels and the bearer token actually landed
    // where promtail expects them. If the template is mis-formatted these
    // lookups will fail loudly.
    let host = parsed["scrape_configs"][0]["static_configs"][0]["labels"]["host"]
        .as_str()
        .expect("host label must be present");
    assert_eq!(host, "asb-test-1");

    let token = parsed["clients"][0]["bearer_token"]
        .as_str()
        .expect("bearer_token must be present");
    assert_eq!(token, "test-token");
}

#[test]
fn test_promtail_yml_escapes_single_quotes() {
    // A token with an embedded single quote would break naive interpolation;
    // verify the YAML still parses and the round-trip preserves the value.
    let cfg = PromtailConfig {
        loki_push_url: "https://loki.example.com/loki/api/v1/push".to_string(),
        loki_push_token: "abc'def".to_string(),
        instance: "asb-quote-1".to_string(),
    };
    let yml = build_promtail_yml(&cfg);
    let parsed: serde_yaml::Value = serde_yaml::from_str(&yml).expect("must be valid YAML");
    assert_eq!(parsed["clients"][0]["bearer_token"].as_str(), Some("abc'def"));
}
