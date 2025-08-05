# This Dockerfile builds the asb binary
# We need to build on Ubuntu because the monero-sys crate requires a bunch of system dependencies
# We will try to use a smaller image here at some point
#
# Latest Ubuntu 24.04 image as of Tue, 05 Aug 2025 15:34:08 GMT
FROM ubuntu:24.04@sha256:a08e551cb33850e4740772b38217fc1796a66da2506d312abe51acda354ff061 AS builder

WORKDIR /build

# Install dependencies
# See .github/workflows/action.yml
RUN apt-get update && \
    apt-get install -y \
        git \
        curl \
        clang \
        libsnappy-dev \
        build-essential \
        cmake \
        libboost-all-dev \
        miniupnpc \
        libunbound-dev \
        graphviz \
        doxygen \
        libunwind8-dev \
        pkg-config \
        libssl-dev \
        libzmq3-dev \
        libsodium-dev \
        libhidapi-dev \
        libabsl-dev \
        libusb-1.0-0-dev \
        libprotobuf-dev \
        protobuf-compiler \
        libnghttp2-dev \
        libevent-dev \
        libexpat1-dev \
        ccache && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

# Install Rust 1.87.0
# See ./rust-toolchain.toml for the Rust version
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --default-toolchain 1.87.0
ENV PATH="/root/.cargo/bin:${PATH}"

COPY . .

# Check that submodules are present (they should be initialized before building)
RUN if [ ! -f "monero-sys/monero/CMakeLists.txt" ]; then \
        echo "ERROR: Submodules not initialized. Run 'git submodule update --init --recursive' before building Docker image."; \
        exit 1; \
    fi

WORKDIR /build/swap

# Act as if we are in a GitHub Actions environment
ENV DOCKER_BUILD=true

RUN cargo build -vv -p swap-asb --bin=asb
RUN cargo build -vv -p swap-controller --bin=asb-controller

# Latest Ubuntu 24.04 image as of Tue, 05 Aug 2025 15:34:08 GMT
FROM ubuntu:24.04@sha256:a08e551cb33850e4740772b38217fc1796a66da2506d312abe51acda354ff061 AS runner

WORKDIR /data

COPY --from=builder /build/target/debug/asb /bin/asb
COPY --from=builder /build/target/debug/asb-controller /bin/asb-controller

ENTRYPOINT ["asb"]