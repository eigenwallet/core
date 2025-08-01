FROM rust:1.87-slim AS builder

WORKDIR /build
RUN apt-get update
RUN apt-get install -y git clang cmake libsnappy-dev
COPY . .
RUN cargo build --release --package rendezvous-server --bin rendezvous-server


FROM debian:bullseye-slim
WORKDIR /data
COPY --from=builder /build/target/release/rendezvous-server /bin/rendezvous-server
EXPOSE 8888
ENTRYPOINT ["rendezvous-server"]
