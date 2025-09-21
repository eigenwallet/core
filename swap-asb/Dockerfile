# This Dockerfile builds the `asb` / `asb-controller` binary
#
# We need to build on Ubuntu because the monero-sys crate requires a bunch of system dependencies
# We will try to use a smaller image here at some point
#
# Latest Ubuntu 24.04 image as of Tue, 05 Aug 2025 15:34:08 GMT
FROM ubuntu:24.04@sha256:a08e551cb33850e4740772b38217fc1796a66da2506d312abe51acda354ff061 AS toolchain

# Install system dependencies
# See .github/actions/set-monero-env/action.yml for required dependencies
# The `--mount` allows us to agressively cache this step.
ENV DEBIAN_FRONTEND=noninteractive
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update && \
    apt-get install -y --allow-change-held-packages \
    autoconf \
    # Left out as we are not compiling for Windows in this Dockerfile
    # nsis \
    # mingw-w64 \
    build-essential \
    pkg-config \
    libtool \
    ccache \
    make \
    cmake \
    gcc \
    g++ \
    git \
    curl \
    lbzip2 \
    gperf \
    g++- \
    libglib2.0-dev \
    # Left out as we are not compiling for Windows in this Dockerfile
    # mingw-w64-x86-64 \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

# See ./rust-toolchain.toml for the Rust version
# This reads from the `rust-toolchain.toml` file to determine which Rust version to install
COPY rust-toolchain.toml /tmp/rust-toolchain.toml
RUN set -eu; \
    RUST_VERSION=$(awk -F\" '/^[[:space:]]*channel[[:space:]]*=/ {print $2; exit}' /tmp/rust-toolchain.toml); \
    echo "Using Rust $RUST_VERSION"; \
    curl -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain "$RUST_VERSION" --profile complete

ENV PATH="/root/.cargo/bin:${PATH}"

# We use cargo-chef to cache our dependency graph (recipe.json)
RUN cargo install cargo-chef --locked

# Probe if all submodules are present
FROM toolchain AS prober
RUN --mount=type=bind,source=.,target=/src,ro \
    test -f /src/monero-sys/monero/CMakeLists.txt

# Analyze the current project to determine the minimum subset of files (Cargo.lock and Cargo.toml manifests)
# required to build it and cache dependencies
FROM toolchain AS planner
COPY . .

# Generate a recipe file
# Contains **ALL** dependencies of the entire Cargo workspace
# We can later use `cargo chef cook --package PACKAGE_TO_BUILD` to install the dependencies for a single one of the packages
RUN cargo chef prepare --recipe-path recipe.json

# Now we build the application
FROM toolchain AS builder

# Build just the external dependencies first
COPY --from=planner /recipe.json recipe.json

# Install dependencies required for `swap-asb`
RUN --mount=type=cache,target=/root/.cargo/registry,sharing=locked \
    --mount=type=cache,target=/root/.cargo/git,sharing=locked \
    cargo chef cook --release --recipe-path recipe.json --package swap-asb

# Install dependencies required for `swap-controller`
RUN --mount=type=cache,target=/root/.cargo/registry,sharing=locked \
    --mount=type=cache,target=/root/.cargo/git,sharing=locked \
    cargo chef cook --release --recipe-path recipe.json --package swap-controller

# Act as if we are in a GitHub Actions environment
ENV DOCKER_BUILD=true

# Now we build our binaries
COPY . .

RUN cargo build --release --locked -vv -p swap-asb --bin=asb
RUN cargo build --release --locked -vv -p swap-controller --bin=asb-controller

# Latest Ubuntu 24.04 image as of Tue, 05 Aug 2025 15:34:08 GMT
FROM ubuntu:24.04@sha256:a08e551cb33850e4740772b38217fc1796a66da2506d312abe51acda354ff061 AS runner

# Install native root certificates
ENV DEBIAN_FRONTEND=noninteractive
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update && \
    apt-get install -y ca-certificates && \
    update-ca-certificates

COPY --from=builder /target/release/asb /bin/asb
COPY --from=builder /target/release/asb-controller /bin/asb-controller

ENTRYPOINT ["asb"]
