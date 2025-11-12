use std::path::PathBuf;
use swap_orchestrator::docker;
use swap_orchestrator::docker::compose::ComposeConfig;
use swap_orchestrator::docker::containers::add_maker_services;

#[tokio::test]
async fn test_config_generates_expected_compose() {
    // 1. Read and parse the example config from tests/output/config.toml
    let test_config_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/output/config.toml");

    let config =
        swap_env::config::Config::read(&test_config_path).expect("Failed to read test config.toml");

    // 2. Determine if we should create full nodes based on the config
    let create_full_bitcoin_node =
        swap_orchestrator::util::should_create_full_bitcoin_node(&config);
    let create_full_monero_node = swap_orchestrator::util::should_create_full_monero_node(&config);

    // 3. Generate the compose config using the actual code path
    let compose_name =
        swap_orchestrator::util::compose_name(config.bitcoin.network, config.monero.network)
            .expect("Failed to generate compose name");

    let mut compose = ComposeConfig::new(compose_name);
    add_maker_services(
        &mut compose,
        config.bitcoin.network,
        config.monero.network,
        create_full_bitcoin_node,
        create_full_monero_node,
    );

    // 4. Build the YAML string
    let generated_yaml = compose.build();

    // 5. Read the expected YAML
    let expected_yaml_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/output/docker-compose.yml");
    let expected_yaml = std::fs::read_to_string(expected_yaml_path)
        .expect("Failed to read expected docker-compose.yml")
        // Replace the git tag to match the current one. It is expected to be different
        .replace(
            "https://github.com/eigenwallet/core.git#833fc0ab24e40555d53f05e6e04728460dab5988",
            docker::images::PINNED_GIT_REPOSITORY,
        );

    // 6. Parse both as serde_yaml::Value to compare structure (ignoring comments/whitespace)
    let generated_value: serde_yaml::Value =
        serde_yaml::from_str(&generated_yaml).expect("Failed to parse generated YAML");
    let expected_value: serde_yaml::Value =
        serde_yaml::from_str(&expected_yaml).expect("Failed to parse expected YAML");

    // 7. Assert they match
    assert_eq!(
        generated_value, expected_value,
        "Generated docker-compose.yml does not match expected output.\n\
         Generated:\n{}\n\n\
         Expected:\n{}",
        generated_yaml, expected_yaml
    );
}
