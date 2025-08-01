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

If the provided secret file doesn't exist, it will be created with a new random secret key.

Run `cargo run --release -- --help` for more options
