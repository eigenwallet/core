# Orchestrator

The `orchestrator` tool helps you setup a secure, reliable and production environment for running an ASB.

The `orchestrator` guides you through a series of prompts to generate a customized [Docker](https://docs.docker.com/) environment using [Docker Compose](https://docs.docker.com/compose/).

## Getting started

Ensure you have [Docker](https://docs.docker.com/engine/install/) and [Docker Compose](https://docs.docker.com/compose/install/) installed on your machine.

To generate the `config.toml` and `docker-compose.yml` files, run:

```bash
./orchestrator
```

```bash
cargo run --bin orchestrator
```

To start the environment, run a command [such as](https://docs.docker.com/reference/cli/docker/compose/up/):

```bash
docker compose up -d
```

To view logs, run commands [such as](https://docs.docker.com/reference/cli/docker/compose/logs/):
```bash
docker compose logs -f --tail 100
docker compose logs -f --tail 100 asb
docker compose logs -f --tail 100 bitcoind
```

Once the `asb` is running properly you can get a shell
```bash
$ docker compose attach asb-controller

ASB Control Shell - Type 'help' for commands, 'quit' to exit

asb> help
Available commands:

Usage: <COMMAND>

Commands:
  check-connection    Check connection to ASB server
  bitcoin-balance     Get Bitcoin balance
  monero-balance      Get Monero balance
  monero-address      Get Monero wallet address
  multiaddresses      Get external multiaddresses
  active-connections  Get active connection count
  get-swaps           Get list of swaps
  help                Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help


Additional shell commands:
  help                 Show this help message
  quit, exit, :q       Exit the shell
asb> 
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
