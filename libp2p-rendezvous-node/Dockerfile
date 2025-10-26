# Latest Rust 1.87.0 image as of Tue, 05 Aug 2025 15:34:08 GMT
FROM rust:1.87.0-bookworm@sha256:251cec8da4689d180f124ef00024c2f83f79d9bf984e43c180a598119e326b84 AS builder

WORKDIR /build

# Install some system dependencies
# TODO: These might not be required?
RUN apt-get update
RUN apt-get install -y git clang cmake libsnappy-dev

# Build the rendezvous server binary
COPY . .
RUN cargo build --release --package rendezvous-node --bin rendezvous-node

# Latest Debian Bookworm image as of Tue, 05 Aug 2025 15:34:08 GMT
FROM debian:bookworm@sha256:b6507e340c43553136f5078284c8c68d86ec8262b1724dde73c325e8d3dcdeba AS runner

# Copy the compiled binary from the previous stage
COPY --from=builder /build/target/release/rendezvous-node /bin/rendezvous-node

EXPOSE 8888

ENTRYPOINT ["rendezvous-node"]
