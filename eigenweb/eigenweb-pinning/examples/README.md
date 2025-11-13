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

## Three-Party Tor Example

The `three-party-tor.rs` example demonstrates the same messaging scenario as `three-party.rs`, but uses Tor hidden services for privacy and anonymity:

- **Carol** and **David**: Pinning servers that listen on Tor hidden services (onion addresses)
- **Alice** and **Bob**: Clients that connect to the servers via their onion addresses

### Key Differences from three-party.rs

1. **Tor Hidden Services**: Servers listen on onion addresses instead of TCP ports
2. **Onion Address Configuration**: Clients specify which servers to connect to using onion addresses (ignoring peer IDs)
3. **Optional Connections**: Clients can connect to one or both servers by omitting server addresses
4. **Privacy**: All communication happens over the Tor network

### Running the Example

You need at least three terminal windows. The servers must be started first to generate their onion addresses.

**Terminal 1 - Start Carol's server:**

```bash
cargo run --example three-party-tor -- --carol
```

Wait for Carol to print her onion address. It will look like:
```
LISTENING ON: /onion3/abcdef1234567890.onion:999
```

**Terminal 2 - Start David's server:**

```bash
cargo run --example three-party-tor -- --david
```

Wait for David to print his onion address.

**Terminal 3 - Start Alice (connecting to both servers):**

```bash
cargo run --example three-party-tor -- --alice \
  --carol-addr /onion3/abcdef1234567890.onion:999 \
  --david-addr /onion3/xyz9876543210.onion:999
```

**Terminal 4 - Start Bob (connecting to both servers):**

```bash
cargo run --example three-party-tor -- --bob \
  --carol-addr /onion3/abcdef1234567890.onion:999 \
  --david-addr /onion3/xyz9876543210.onion:999
```

### Optional Server Connections

Clients can connect to only one server by omitting the other:

**Alice connecting to Carol only:**

```bash
cargo run --example three-party-tor -- --alice \
  --carol-addr /onion3/abcdef1234567890.onion:999
```

**Bob connecting to David only:**

```bash
cargo run --example three-party-tor -- --bob \
  --david-addr /onion3/xyz9876543210.onion:999
```

### Usage

Once all parties are running and connected:

- In Alice's terminal, type a message and press Enter to send it to Bob
- In Bob's terminal, type a message and press Enter to send it to Alice
- Messages are relayed through the Tor hidden services
- All communication is private and happens over the Tor network

### Notes

- Tor bootstrap can take 10-30 seconds on first run
- Onion service publication can take an additional 30-60 seconds
- Ensure you copy the full onion addresses from the server terminals
- At least one server address must be provided for clients to connect
