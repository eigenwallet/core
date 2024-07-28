#![doc = include_str!("../README.md")]

/// Implementation of an in-process [`arti-client`](https://crates.io/crates/arti-client)-based `TorProvider`
#[cfg(feature = "arti-client-tor-provider")]
pub mod arti_client_tor_client;
#[cfg(feature = "legacy-tor-provider")]
/// Censorship circumvention configuration for pluggable-transports and bridge settings
pub mod censorship_circumvention;
/// Implementation of an out-of-process legacy [c-tor daemon](https://gitlab.torproject.org/tpo/core/tor)-based `TorProvider`
#[cfg(feature = "legacy-tor-provider")]
pub mod legacy_tor_client;
#[cfg(feature = "legacy-tor-provider")]
mod legacy_tor_control_stream;
#[cfg(feature = "legacy-tor-provider")]
mod legacy_tor_controller;
#[cfg(feature = "legacy-tor-provider")]
mod legacy_tor_process;
#[cfg(feature = "legacy-tor-provider")]
mod legacy_tor_version;
/// Implementation of a local, in-process, mock `TorProvider` for testing.
#[cfg(feature = "mock-tor-provider")]
pub mod mock_tor_client;
#[cfg(feature = "legacy-tor-provider")]
/// Proxy settings
pub mod proxy;
/// Tor-specific cryptographic primitives, operations, and conversion functions.
pub mod tor_crypto;
/// Traits and types for connecting to the Tor Network.
pub mod tor_provider;
