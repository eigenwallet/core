#![doc = include_str!("../README.md")]

/// Implementation of an in-process [`arti-client`](https://crates.io/crates/arti-client)-based `TorProvider`
#[cfg(feature = "arti-client-tor-provider")]
pub mod arti_client_tor_client;
/// Implementation of an out-of-process [`arti`](https://crates.io/crates/arti)-based `TorProvider`
#[cfg(feature = "arti-tor-provider")]
pub mod arti_tor_client;
#[cfg(feature = "arti-tor-provider")]
pub mod arti_process;
/// Censorship circumvention configuration for pluggable-transports and bridge settings
#[cfg(feature = "legacy-tor-provider")]
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
/// Legacy c-tor daemon version.
#[cfg(feature = "legacy-tor-provider")]
pub mod legacy_tor_version;
/// Implementation of a local, in-process, mock `TorProvider` for testing.
#[cfg(feature = "mock-tor-provider")]
pub mod mock_tor_client;
/// Proxy settings
#[cfg(feature = "legacy-tor-provider")]
pub mod proxy;
/// Tor-specific cryptographic primitives, operations, and conversion functions.
pub mod tor_crypto;
/// Traits and types for connecting to the Tor Network.
pub mod tor_provider;
