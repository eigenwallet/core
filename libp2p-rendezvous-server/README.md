# Standalone Rendezvous Server

A standalone libp2p [rendezvous server](https://github.com/libp2p/specs/tree/master/rendezvous) binary.

## Usage

Build the binary:

```
cargo build --release
```

Run the `libp2p-rendezvous-server`:

```
cargo run --release
```

The server will use default values:

- Secret file: `rendezvous-server-secret.key` (created automatically if it doesn't exist)
- Listen port: `8888`

You can customize these with:

```
cargo run --release -- --secret-file <PATH-TO-SECRET-FILE> --listen-tcp <PORT>
```

## Tor Onion Service Support

The rendezvous server can also listen on a Tor onion service for enhanced privacy:

```
cargo run --release -- --onion
```

This will:

- Bootstrap a connection to the Tor network
- Create a new onion service
- Listen on both TCP (port 8888) and the onion address
- Print the onion address in the logs

You can specify a custom port for the onion service:

```
cargo run --release -- --onion --onion-port 9999
```

## Options

If the provided secret file doesn't exist, it will be created with a new random secret key.

Run `cargo run --release -- --help` for all available options:

- `--secret-file`: Path to the secret key file
- `--listen-tcp`: TCP port to listen on (default: 8888)
- `--onion`: Enable Tor onion service
- `--onion-port`: Port for the onion service (default: 8888)
- `--json`: Format logs as JSON
- `--no-timestamp`: Don't include timestamp in logs
