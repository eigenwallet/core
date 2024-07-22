# Tor-Interface

Developer-friendly crate providing connectivity to the [Tor Network](https://en.wikipedia.org/wiki/Tor_(network)) and functionality for interacting with Tor-specific cryptographic types.

This crate is *not* meant to be a general purpose Tor Controller nor does it aim to expose all of the functionality of the underlying Tor implementations. This crate also does not implement any of the Tor Network functionality itself, instead wrapping lower-level implementations.

## Overview

The `tor-interface` crate provides the `TorProvider` trait with 3 concrete implementations:

- ArtiClientTorClient: an experimental wrapper around the [`arti-client`](https://crates.io/crates/arti-client) crate; enabled using the **arti-client-tor-provider** feature flag.
- LegacyTorClient: a wrapper around either an owned or system-provided legacy c-tor daemon (aka 'little-t tor') with some basic configuration options; enabled using the **legacy-tor-provider** feature flag.
- MockTorClient: an in-process, mock implementation which makes no actual connections outside of localhost; enabled with the **mock-tor-provider** feature flag.

The `TorProvider` trait defines methods for connecting to various types of target addresses (ip, domains, and onion-services) and for creating onion-services.

## ⚠ Warning ⚠

The **arti-client-tor-provider** feature is experimental is not fully implemented. It also depends on the [`arti-client`](https://crates.io/crates/arti-client) crate which is still under active development and is generally not yet ready for production use.

## Usage

The following code snippet creates a `LegacyTorClient` which starts a bundled tor daemon, bootstraps, and attempts to connect to [www.example.com](www.example.com).

```rust
# use std::str::FromStr;
# use std::net::TcpStream;
# use tor_interface::legacy_tor_client::{LegacyTorClient, LegacyTorClientConfig};
# use tor_interface::tor_provider::{OnionStream, TargetAddr, TorEvent, TorProvider};
# return;
// construct legacy tor client config
let tor_path = std::path::PathBuf::from_str("/usr/bin/tor").unwrap();
let mut data_path = std::env::temp_dir();
data_path.push("tor_data");

let tor_config = LegacyTorClientConfig::BundledTor {
    tor_bin_path: tor_path,
    data_directory: data_path,
    proxy_settings: None,
    allowed_ports: None,
    pluggable_transports: None,
    bridge_lines: None,
};
// create client from config
let mut tor_client = LegacyTorClient::new(tor_config).unwrap();

// bootstrap tor
let mut bootstrap_complete = false;
while !bootstrap_complete {
    for event in tor_client.update().unwrap().iter() {
        match event {
            TorEvent::BootstrapComplete => {
                bootstrap_complete = true;
            },
            _ => {},
        }
    }
}

// connect to example.com
let target_addr = TargetAddr::from_str("www.example.com:80").unwrap();
let mut stream: OnionStream = tor_client.connect(target_addr, None).unwrap();
// and convert to a std::net::TcpStream
let stream: TcpStream = stream.into();
```
