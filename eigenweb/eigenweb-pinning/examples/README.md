# Examples

This directory contains example implementations demonstrating how to use the eigenweb-pinning library.

**Note:** The examples in this directory are AI-generated for demonstration purposes. The core library code (`src/`) is hand-written and should not be modified by AI tools.

## Three-Party Example

The `three-party.rs` example demonstrates a complete messaging scenario with three participants:

- **Alice**: A client who sends messages to Bob
- **Bob**: A client who sends messages to Alice
- **Carol**: A pinning server that stores and relays messages between Alice and Bob

### How It Works

1. Carol runs a pinning server that listens on port 9000
2. Alice and Bob connect to Carol's server as clients
3. Alice and Bob can send messages to each other through Carol's server
4. Messages are automatically fetched in the background from the server

### Running the Example

You need three terminal windows to run this example:

**Terminal 1 - Start Carol (the server):**

```bash
cargo run --example three-party -- --carol
```

**Terminal 2 - Start Alice (client):**

```bash
cargo run --example three-party -- --alice
```

**Terminal 3 - Start Bob (client):**

```bash
cargo run --example three-party -- --bob
```

### Usage

Once all three parties are running and connected:

- In Alice's terminal, type a message and press Enter to send it to Bob
- In Bob's terminal, type a message and press Enter to send it to Alice
- Messages are queued on Carol's server and automatically fetched by the recipients
