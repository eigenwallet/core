# Standalone Rendezvous Server

A standalone libp2p [rendezvous server](https://github.com/libp2p/specs/tree/master/rendezvous) binary.

## Usage

Build the binary:

```
cargo build --release
```

Run the `libp2p-rendezvous-node`:

```
cargo run --release
```

The server will use default values:

- Data directory: `./rendezvous-data` (contains identity and Tor state, created automatically if it doesn't exist)
- Listen port: `8888`

You can customize these with:

```
cargo run --release -- --data-dir <PATH-TO-DATA-DIR> --port <PORT>
```

## Tor Onion Service Support

By default, the rendezvous server listens on both TCP and a Tor onion service for enhanced privacy. This will:

- Bootstrap a connection to the Tor network
- Create a new onion service
- Listen on both TCP (port 8888) and the onion address
- Print the onion address in the logs

To disable the onion service and use only TCP:

```
cargo run --release -- --no-onion
```

## Options

The data directory stores the LibP2P identity and Tor state. If it doesn't exist, it will be created along with a new random identity key.

Run `cargo run --release -- --help` for all available options:

- `--data-dir`: Path to the data directory (default: `./rendezvous-data`)
- `--port`: Port to listen on for both TCP and onion service (default: `8888`)
- `--no-onion`: Disable Tor onion service (enabled by default)
