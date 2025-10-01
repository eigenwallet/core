///! This module describes **how to build** the containers
/// This means either:
/// 1. Pulling from a registry (pinned to a hash)
/// 2. Building from source from a specific git hash (pinned to a hash)

/// At compile time, we determine the git repository and commit hash
/// This is then burned into the binary as a static string
/// If the Git hash doesn't match, Docker will fail to build the image
pub static PINNED_GIT_REPOSITORY: &str = concat!(
    "https://github.com/eigenwallet/core.git#",
    env!("VERGEN_GIT_SHA")
);

/// All of these images are pinned to a specific commit
/// This ensures that the images cannot be altered by the registry

/// monerod v0.18.4.1 (https://github.com/sethforprivacy/simple-monerod-docker/pkgs/container/simple-monerod/471968653)
pub static MONEROD_IMAGE: &str = "ghcr.io/sethforprivacy/simple-monerod@sha256:f30e5706a335c384e4cf420215cbffd1196f0b3a11d4dd4e819fe3e0bca41ec5";

/// electrs v0.10.9 (https://hub.docker.com/layers/getumbrel/electrs/v0.10.9/images/sha256-738d066836953c28936eab59fd87bf5f940d457260d0d13cfc99b06513248fee)
pub static ELECTRS_IMAGE: &str =
    "getumbrel/electrs@sha256:622657fbdc7331a69f5b3444e6f87867d51ac27d90c399c8bf25d9aab020052b";

/// bitcoind v28.1 (https://hub.docker.com/layers/getumbrel/bitcoind/v28.1/images/sha256-8a20dc4efd799c17fd20f27cc62a36d1ef157e2ef074a898eae88c712b8d0e24)
pub static BITCOIND_IMAGE: &str =
    "getumbrel/bitcoind@sha256:c565266ea302c9ab2fc490f04ff14e584210cde3d0d991b8309157e5dfae9e8d";

/// alpine 3.22.1 (https://hub.docker.com/layers/library/alpine/3.22.1/images/sha256-0a88b42ba69d6b900848f9cb9151587bb82827d0aecfa222e51981fad97b5b9a)
pub static ASB_TRACING_LOGGER_IMAGE: &str =
    "alpine@sha256:4bcff63911fcb4448bd4fdacec207030997caf25e9bea4045fa6c8c44de311d1";
