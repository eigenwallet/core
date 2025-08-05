# Latest Rust 1.87.0 image as of Tue, 05 Aug 2025 15:34:08 GMT
# This needs to be built from the root of the Cargo workspace (root of git repo)
FROM rust:1.87.0-bookworm@sha256:251cec8da4689d180f124ef00024c2f83f79d9bf984e43c180a598119e326b84 AS builder

WORKDIR /build

# Copy the entire Cargo workspace
COPY . .

RUN cargo build --locked --bin asb-controller --release

# Latest Debian Bookworm image as of Tue, 05 Aug 2025 15:34:08 GMT
FROM debian:bookworm@sha256:b6507e340c43553136f5078284c8c68d86ec8262b1724dde73c325e8d3dcdeba AS runner

COPY --from=builder /build/target/release/asb-controller /usr/local/bin/asb-controller

ENTRYPOINT ["/usr/local/bin/asb-controller"]