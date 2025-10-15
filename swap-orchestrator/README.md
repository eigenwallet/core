# Orchestrator

The `orchestrator` tool helps you setup a secure, reliable and production environment for running an ASB. It guides you through a series of prompts to generate a customized [Docker](https://docs.docker.com/) environment using [Docker Compose](https://docs.docker.com/compose/).

## Getting started

Ensure you have [Docker](https://docs.docker.com/engine/install/) and [Docker Compose](https://docs.docker.com/compose/install/) installed on your machine.

If you're not compiling the `orchestrator` from source you can grab the latest [release](https://github.com/eigenwallet/core/releases) from the download section.

> [!TIP]
> **Linux x86_64 quick install**
>
> Run the commands below if you're on a Linux x86 server.
> It'll download the latest archive from Github, extract the binary and make it executable.
> You can also run this to update a pre-existing `orchestrator` install.
>
> Download the archive which contains the pre-compiled binary:
>
> ```bash
> name="$(
>   curl -fsSL https://api.github.com/repos/eigenwallet/core/releases/latest \
>   | grep -oE '"name":\s*"orchestrator_[^"]*_Linux_x86_64\.tar"' \
>   | head -n1 | cut -d'"' -f4
> )"
> curl -fL -o "orchestrator_linux.tar" "https://github.com/eigenwallet/core/releases/latest/download/$name"
> ```
>
> Extract the archive to get the `orchestrator` binary
>
> ```bash
> tar -xf ./orchestrator_linux.tar
> ```
>
> Make the binary executable
>
> ```bash
> chmod +x orchestrator
> ```

Run the command below to start the wizard. It’ll guide you through a bunch of questions to generate the `config.toml` file and the `docker-compose.yml` file based on your needs. You can always modify the `config.toml` later on to modify specific things about your `asb` like the minimum swap amount or the configured markup.

```bash
./orchestrator
```

To build the images, run this command. Also run this after upgrading the `orchestrator` and re-generating `docker-compose.yml`:

```bash
docker compose build --no-cache # --no-cache fixes a git caching issue (error: tag clobbered)
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
docker compose logs -f --tail 100 electrs
```

To view high-verbosity logs of the asb, peek inside the `asb-tracing-logger` container by running commands [such as](https://docs.docker.com/reference/cli/docker/compose/logs/):

```bash
docker compose logs -f --tail 100 asb-tracing-logger
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

<img width="1364" height="709" alt="image" src="https://github.com/user-attachments/assets/cdc47e64-7ffb-4da9-811a-d020b1b20bd2" />

## Why Docker?

Your ASB will potentially be managing fairly large amounts of funds. This means you have to keep it secure and running reliably. Docker handles some of this for you.

Most importantly Docker provides:

- **System Isolation**: Containers are isolated from each other on the operating system level. If one of the nodes were to be affected by a vulnerability, your funds are still safe.
- **Network Isolation**: Docker only exposes the peer-to-peer port of your ASB to the outside world. Bitcoin, Monero and Electrum containers are only allowed outbound connections. The RPC control port of your ASB is only accessible within Docker inside of an internal network, accessible only to the `asb-controller` container.
- **Reproducibility**: A virtual environment is created for each container. This means that quirks on your system (e.g outdated `glibc`) will not become an issue.
- **Building from source**: Building from source is as simple as passing a flag to Docker. You do not have to install any dependencies on your system.

## Demo

[![demo](https://github.com/user-attachments/assets/21d82a48-8f2e-41dc-9020-9439a98bd543)](https://asciinema.org/a/tKE8IPyP5dI9KjmPGhBcBPtWg)
