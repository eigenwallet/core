#![doc = include_str!("../README.md")]

#[cfg(feature = "arti-client-tor-provider")]
pub mod arti_client_tor_client;
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
#[cfg(feature = "mock-tor-provider")]
pub mod mock_tor_client;
#[cfg(feature = "legacy-tor-provider")]
pub mod proxy;
/// Tor-specific cryptographic primitives, operations, and conversion functions.
pub mod tor_crypto;
pub mod tor_provider;
