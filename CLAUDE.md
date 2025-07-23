# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is eigenwallet (formerly UnstoppableSwap), a monorepo containing XMR-BTC atomic swap protocol implementation with multiple components:

- **swap/** - Core Rust library and binaries (asb, swap CLI) implementing the atomic swap protocol
- **src-gui/** - React/TypeScript frontend GUI using Vite
- **src-tauri/** - Tauri wrapper providing native desktop app integration
- **monero-sys/** - Rust FFI bindings for Monero C++ libraries
- **monero-rpc-pool/** - Monero RPC connection pooling service
- Various utility crates (swap-feed, swap-fs, swap-serde, etc.)

The project implements atomic swaps between Bitcoin and Monero cryptocurrencies using a peer-to-peer protocol.

## Build and Development Commands

### Core Development Commands (use `just` command runner)
- `just help` - Show all available commands
- `just tests` - Run Rust test suite with cargo-nextest
- `just clippy` - Run linting checks
- `just fmt` - Format code using dprint

### GUI Development
- `just gui_install` - Install frontend dependencies (cd src-gui && yarn install)
- `just web` - Start Vite dev server (cd src-gui && yarn dev)
- `just tauri` - Start Tauri app in testnet mode
- `just tauri-mainnet` - Start Tauri app in mainnet mode
- `just tauri-mobile` - Start Tauri app with mobile screen ratio in testnet mode
- `just tauri-mobile-mainnet` - Start Tauri app with mobile screen ratio in mainnet mode
- `just gui` - Run both web server and Tauri app concurrently
- `just gui-mobile` - Run both web server and Tauri app with mobile layout
- `just gui_build` - Build the GUI for production
- `just bindings` - Generate TypeScript bindings from Rust types
- `just check_gui` - Run ESLint and TypeScript checks

### Swap Binaries
- `just swap` - Build ASB and swap binaries
- `just asb-testnet` - Run ASB (Automated Swap Backend) on testnet

### Testing
- Use `cargo nextest run` for running tests (requires `cargo install cargo-nextest`)
- `just test_monero_sys` - Test Monero FFI bindings
- `just test-ffi` - Test FFI bindings with sanitizers

## Architecture

### Core Components

1. **Protocol Layer (swap/src/protocol/)**
   - Alice: Market maker (sells XMR, receives BTC)
   - Bob: Taker (buys XMR, sends BTC)
   - State machines managing swap phases

2. **Network Layer (swap/src/network/)**
   - libp2p-based P2P communication
   - Quote negotiation, swap setup, encrypted signatures
   - Rendezvous point discovery

3. **Blockchain Integration**
   - **Bitcoin (swap/src/bitcoin/)**: Lock, redeem, refund, punish transactions
   - **Monero (swap/src/monero/)**: Wallet integration via RPC

4. **Database Layer (swap/src/database/)**
   - SQLite-based persistence for swap state
   - Separate schemas for Alice and Bob

5. **GUI Architecture**
   - React/Redux frontend in TypeScript
   - Tauri backend providing native desktop integration
   - Type-safe communication via generated bindings using typeshare

### Key Directories
- `swap/src/bin/` - Main binaries (asb.rs, swap.rs)
- `swap/src/cli/` - CLI implementation and API layer
- `swap/src/asb/` - Automated Swap Backend implementation
- `monero-sys/` - Monero C++ library Rust bindings
- `src-gui/src/components/` - React components
- `src-gui/src/store/` - Redux state management

## Development Workflow

### Setting up Development Environment
1. Install Rust toolchain (latest stable required, 1.74+ supported)
2. Install cargo-nextest: `cargo install cargo-nextest`
3. Install dprint: `cargo install dprint@0.50.0`
4. Install typeshare: `cargo install typeshare-cli`
5. Install Node.js and yarn for frontend development
6. For GUI development: Install Tauri CLI

### Working with Monero Integration
- Monero C++ codebase is included as submodule in monero-sys/monero/
- Use `just update_submodules` to initialize/update submodules
- `just monero_sys` builds the Rust bindings
- FFI bindings require specific system dependencies (see brew script for macOS)

### GUI Development Specific
- TypeScript bindings are auto-generated from Rust types using typeshare
- Run `just bindings` after changing Rust types that are shared with frontend
- Use `yarn run check-bindings` to verify bindings are up to date
- GUI connects to Tauri backend which wraps the swap library

### Testing and CI
- All code must pass `dprint check` and `cargo clippy` 
- Run full test suite with `cargo nextest run`
- Integration tests require Docker network setup
- Use `just docker-prune-network` if integration tests fail

## Important Constraints

### Dependency Management
- **CRITICAL**: Never upgrade dependency versions without explicit confirmation (enforced by .cursor/rules)
- Use workspace dependencies defined in root Cargo.toml
- Patches are applied for specific library versions

### Code Standards
- Forbids unsafe code (#![forbid(unsafe_code)])
- Extensive clippy lints enabled
- Use semantic linebreaks in documentation
- Atomic commits with clear messages following conventional commit style

### Network and Protocol
- Supports both testnet and mainnet operations
- ASB runs as long-running daemon, CLI for individual swaps
- P2P discovery via rendezvous points
- Protocol implements paper: https://arxiv.org/abs/2101.12332

## Common Tasks

### Adding New Rust Types Shared with GUI
1. Add typeshare derive to the Rust struct/enum
2. Run `just bindings` to regenerate TypeScript definitions
3. Update frontend code to use new types

### Mobile Development
- Use `just gui-mobile` to run the app in mobile screen ratio (390x844px)
- Mobile layout uses bottom navigation instead of sidebar
- All modals display full-screen on mobile
- Components automatically adapt using `useIsMobile()` hook

### Debugging GUI
- Use standalone React DevTools: `npx react-devtools`
- Use Redux DevTools: `npx redux-devtools --hostname=localhost --port=8098 --open`

### Running Swaps
- For testing: Use `--testnet` flag on both asb and swap binaries
- CLI: `./swap --testnet buy-xmr --receive-address <XMR_ADDR> --change-address <BTC_ADDR> --seller <MULTIADDR>`
- ASB: `./asb --testnet start`