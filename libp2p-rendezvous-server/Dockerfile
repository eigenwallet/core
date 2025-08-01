# Build this Dockerfile from the workspace root:
# docker build -f libp2p-rendezvous-server/Dockerfile .
FROM rust:1.87-slim AS builder

WORKDIR /build
RUN apt-get update
RUN apt-get install -y git clang cmake libsnappy-dev
COPY . .
RUN cargo build --release --package libp2p-rendezvous-server --bin libp2p-rendezvous-server


FROM debian:bullseye-slim
WORKDIR /data
COPY --from=builder /build/target/release/libp2p-rendezvous-server /bin/libp2p-rendezvous-server
EXPOSE 8888
ENTRYPOINT ["libp2p-rendezvous-server"]
