# eigenweb-node

A standalone libp2p [eigenweb-node](https://github.com/libp2p/specs/tree/master/rendezvous) binary.

## Usage

Build the binary:

```
cargo build --release
```

<<<<<<<< HEAD:eigenweb/eigenweb-node/README.md
Run the `eigenweb-node`:
========
Run the `libp2p-rendezvous-node`:
>>>>>>>> master:libp2p-rendezvous-node/README.md

```
cargo run --release
```

The eigenweb-node will use default values:

- Secret file: `eigenweb-node-secret.key` (created automatically if it doesn't exist)
- Listen port: `8888`

You can customize these with:

```
cargo run --release -- --secret-file <PATH-TO-SECRET-FILE> --listen-tcp <PORT>
```

## Tor Onion Service Support

By default, the eigenweb-node listens on both TCP and a Tor onion service for enhanced privacy. This will:

- Bootstrap a connection to the Tor network
- Create a new onion service
- Listen on both TCP (port 8888) and the onion address
- Print the onion address in the logs

To disable the onion service and use only TCP:

```
cargo run --release -- --no-onion
```

You can specify a custom port for the onion service:

```
cargo run --release -- --onion-port 9999
```

## Options

If the provided secret file doesn't exist, it will be created with a new random secret key.

Run `cargo run --release -- --help` for all available options:

- `--secret-file`: Path to the secret key file
- `--listen-tcp`: TCP port to listen on (default: 8888)
- `--no-onion`: Disable Tor onion service (enabled by default)
- `--onion-port`: Port for the onion service (default: 8888)
- `--json`: Format logs as JSON
- `--no-timestamp`: Don't include timestamp in logs
