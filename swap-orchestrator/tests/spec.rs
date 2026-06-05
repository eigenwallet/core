#![allow(unused_crate_dependencies)]

use swap_orchestrator::compose::{
    CloudflaredConfig, IntoSpec, MetricsConfig, OrchestratorDirectories, OrchestratorImage,
    OrchestratorImages, OrchestratorInput, OrchestratorNetworks, OrchestratorPorts, PromtailConfig,
    build_prometheus_agent_yml, build_promtail_yml,
};
use swap_orchestrator::images;

fn make_input(
    want_tor: bool,
    cloudflared: Option<CloudflaredConfig>,
    promtail: Option<PromtailConfig>,
    metrics: Option<MetricsConfig>,
) -> OrchestratorInput {
    let source_build_context = images::source_build_context(None);
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
            rendezvous_node: OrchestratorImage::Build(images::rendezvous_node_image_from_source(
                &source_build_context,
            )),
            asb: OrchestratorImage::Build(images::asb_image_from_source(&source_build_context)),
            asb_controller: OrchestratorImage::Build(images::asb_controller_image_from_source(
                &source_build_context,
            )),
            asb_tracing_logger: OrchestratorImage::Registry(
                images::ASB_TRACING_LOGGER_IMAGE.to_string(),
            ),
            cloudflared: OrchestratorImage::Registry(images::CLOUDFLARED_IMAGE.to_string()),
            promtail: OrchestratorImage::Registry(images::PROMTAIL_IMAGE.to_string()),
            docker_socket_proxy: OrchestratorImage::Registry(
                images::DOCKER_SOCKET_PROXY_IMAGE.to_string(),
            ),
            cadvisor: OrchestratorImage::Registry(images::CADVISOR_IMAGE.to_string()),
            prometheus_agent: OrchestratorImage::Registry(images::PROMETHEUS_IMAGE.to_string()),
        },
        directories: OrchestratorDirectories {
            asb_data_dir: std::path::PathBuf::from(swap_orchestrator::compose::ASB_DATA_DIR),
        },
        want_tor,
        cloudflared,
        promtail,
        metrics,
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

fn sample_metrics_config() -> MetricsConfig {
    MetricsConfig {
        remote_write_url: "https://loki-asb-logs.example.com/api/v1/write".to_string(),
        token: "test-token".to_string(),
        instance: "asb-test-1".to_string(),
    }
}

#[test]
fn test_orchestrator_spec_generation() {
    // `to_spec` runs `validate_compose` internally, so generating each
    // variant is enough to catch indentation regressions in the optional
    // tor / cloudflared / promtail segments.
    let _ = make_input(false, None, None, None).to_spec();
    let _ = make_input(true, None, None, None).to_spec();
    let _ = make_input(false, Some(sample_cloudflared_config()), None, None).to_spec();
    let _ = make_input(true, Some(sample_cloudflared_config()), None, None).to_spec();
    let compose = make_input(false, None, Some(sample_promtail_config()), None).to_spec();
    let _ = make_input(true, None, Some(sample_promtail_config()), None).to_spec();

    // promtail's docker SD needs the networks API, not just containers, or
    // discovery 403s on /networks and no node logs ship.
    assert!(compose.contains("NETWORKS=1"));
    let _ = make_input(
        true,
        Some(sample_cloudflared_config()),
        Some(sample_promtail_config()),
        None,
    )
    .to_spec();

    // With metrics enabled, both cadvisor and the prometheus agent must appear.
    let metrics_compose = make_input(
        true,
        Some(sample_cloudflared_config()),
        Some(sample_promtail_config()),
        Some(sample_metrics_config()),
    )
    .to_spec();
    assert!(metrics_compose.contains("container_name: cadvisor"));
    assert!(metrics_compose.contains("container_name: prometheus-agent"));
    assert!(metrics_compose.contains("prometheus-agent-data:"));

    // Without metrics, neither service is generated.
    let plain = make_input(false, None, None, None).to_spec();
    assert!(!plain.contains("cadvisor"));
    assert!(!plain.contains("prometheus-agent"));
}

#[test]
fn test_gh_token_inlined_into_build_context() {
    let context = images::source_build_context(Some("ghp_exampletoken"));
    assert!(context.starts_with("https://ghp_exampletoken@github.com/"));

    // A spec built from the authenticated context must still validate, and the
    // token must reach the build attribute of every source-built service.
    let mut input = make_input(false, None, None, None);
    input.images.asb = OrchestratorImage::Build(images::asb_image_from_source(&context));
    input.images.asb_controller =
        OrchestratorImage::Build(images::asb_controller_image_from_source(&context));
    input.images.rendezvous_node =
        OrchestratorImage::Build(images::rendezvous_node_image_from_source(&context));

    let compose = input.to_spec();
    assert_eq!(compose.matches("ghp_exampletoken@github.com").count(), 3);
}

#[test]
fn test_source_build_context_without_token_is_clean() {
    let context = images::source_build_context(None);
    assert!(context.starts_with("https://github.com/"));
    assert!(!context.contains('@'));
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

#[test]
fn test_promtail_yml_ships_node_container_logs() {
    let yml = build_promtail_yml(&sample_promtail_config());
    let parsed: serde_yaml::Value =
        serde_yaml::from_str(&yml).expect("promtail.yml must be valid YAML");

    let node_job = parsed["scrape_configs"]
        .as_sequence()
        .expect("scrape_configs must be a list")
        .iter()
        .find(|job| job["job_name"].as_str() == Some("node"))
        .expect("a `node` scrape job must be present");

    // The node logs are read through the docker-socket-proxy, not a file path.
    assert_eq!(
        node_job["docker_sd_configs"][0]["host"].as_str(),
        Some("tcp://docker-socket-proxy:2375")
    );

    // Only the three daemon containers are shipped.
    let keep = node_job["relabel_configs"][0]["regex"]
        .as_str()
        .expect("keep regex must be present");
    assert!(keep.contains("bitcoind") && keep.contains("monerod") && keep.contains("electrs"));

    // Node logs carry the same `host` label as the asb logs so a whole
    // deployment selects with one query.
    let host_relabel = node_job["relabel_configs"]
        .as_sequence()
        .expect("relabel_configs must be a list")
        .iter()
        .find(|rc| rc["target_label"].as_str() == Some("host"))
        .expect("a host relabel must be present");
    assert_eq!(host_relabel["replacement"].as_str(), Some("asb-test-1"));
}

#[test]
fn test_prometheus_agent_yml_is_valid_and_wired() {
    let yml = build_prometheus_agent_yml(&sample_metrics_config());
    let parsed: serde_yaml::Value =
        serde_yaml::from_str(&yml).expect("prometheus.yml must be valid YAML");

    // The host external label must match the Promtail instance so metrics and
    // logs share one selector in Grafana.
    assert_eq!(
        parsed["global"]["external_labels"]["host"].as_str(),
        Some("asb-test-1")
    );

    // The agent scrapes the local cadvisor and pushes to the remote endpoint
    // with the shared bearer token.
    assert_eq!(
        parsed["scrape_configs"][0]["static_configs"][0]["targets"][0].as_str(),
        Some("cadvisor:8080")
    );
    let remote = &parsed["remote_write"][0];
    assert_eq!(
        remote["url"].as_str(),
        Some("https://loki-asb-logs.example.com/api/v1/write")
    );
    assert_eq!(remote["bearer_token"].as_str(), Some("test-token"));
}
