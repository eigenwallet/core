# Orchestration

This tool helps you setup a secure, reliable and production environment for running an ASB.

The `orchestrator` guides you through a series of prompts to generate a customized [Docker](https://docs.docker.com/) environment using [Docker Compose](https://docs.docker.com/compose/).

## Getting started

To generate the `config.toml` and `docker-compose.yml` files, run:

```bash
cargo run --bin orchestrator
```

To start the environment, run:

```bash
docker compose up -d
```

## Architecture

The `orchestrator` generates a `docker-compose.yml` file that includes the following containers:

- `asb`: Accepts connections from the outside world. Provides sell offers to the outside world. Manages your Bitcoin and Monero wallet. Controls the swap process.
- `asb-controller`: Shell to send control commands to the `asb` container.
- `bitcoind`: Bitcoin node.
- `electrs`: Electrum server.
- `monerod`: Monero node.

## Why Docker?

Your ASB will potentially be managing fairly large amounts of funds. This means you have to keep it secure and running reliably. Docker handles some of this for you.

Most importantly Docker provides:

- **System Isolation**: Containers are isolated from each other on the operating system level. If one of the nodes were to be affected by a vulnerability, your funds are still safe.
- **Network Isolation**: Docker only exposes the peer-to-peer port of your ASB to the outside world. Bitcoin, Monero and Electrum containers are only allowed outbound connections. The RPC control port of your ASB is only accessible within Docker inside of an internal network, accessible only to the `asb-controller` container.
- **Reproducibility**: A virtual environment is created for each container. This means that quirks on your system (e.g outdated `glibc`) will not become an issue.
- **Building from source**: Building from source is as simple as passing a flag to Docker. You do not have to install any dependencies on your system.
