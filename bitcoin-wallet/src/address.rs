use anyhow::{Context, Result};
use bitcoin::{
    address::{NetworkChecked, NetworkUnchecked},
    Address,
};
use serde::Serialize;
use std::str::FromStr;

#[derive(thiserror::Error, Debug, Clone, Copy, PartialEq, Serialize)]
#[error("Invalid Bitcoin address provided, expected address on network {expected:?}  but address provided is on {actual:?}")]
pub struct BitcoinAddressNetworkMismatch {
    #[serde(with = "swap_serde::bitcoin::network")]
    expected: bitcoin::Network,
    #[serde(with = "swap_serde::bitcoin::network")]
    actual: bitcoin::Network,
}

pub fn parse(addr_str: &str) -> Result<bitcoin::Address<NetworkUnchecked>> {
    let address = bitcoin::Address::from_str(addr_str)?;

    if address.assume_checked_ref().address_type() != Some(bitcoin::AddressType::P2wpkh) {
        anyhow::bail!("Invalid Bitcoin address provided, only bech32 format is supported!")
    }

    Ok(address)
}

/// Parse the address and validate the network.
pub fn parse_and_validate_network(
    address: &str,
    expected_network: bitcoin::Network,
) -> Result<bitcoin::Address> {
    let addres = bitcoin::Address::from_str(address)?;
    let addres = addres.require_network(expected_network).with_context(|| {
        format!("Bitcoin address network mismatch, expected `{expected_network:?}`")
    })?;
    Ok(addres)
}

/// Parse the address and validate the network.
pub fn parse_and_validate(address: &str, is_testnet: bool) -> Result<bitcoin::Address> {
    let expected_network = if is_testnet {
        bitcoin::Network::Testnet
    } else {
        bitcoin::Network::Bitcoin
    };
    parse_and_validate_network(address, expected_network)
}

/// Validate the address network.
pub fn validate(
    address: Address<NetworkUnchecked>,
    is_testnet: bool,
) -> Result<Address<NetworkChecked>> {
    let expected_network = if is_testnet {
        bitcoin::Network::Testnet
    } else {
        bitcoin::Network::Bitcoin
    };
    validate_network(address, expected_network)
}

/// Validate the address network.
pub fn validate_network(
    address: Address<NetworkUnchecked>,
    expected_network: bitcoin::Network,
) -> Result<Address<NetworkChecked>> {
    address
        .require_network(expected_network)
        .context("Bitcoin address network mismatch")
}

/// Validate the address network even though the address is already checked.
pub fn revalidate_network(
    address: Address,
    expected_network: bitcoin::Network,
) -> Result<Address> {
    address
        .as_unchecked()
        .clone()
        .require_network(expected_network)
        .context("bitcoin address network mismatch")
}

/// Validate the address network even though the address is already checked.
pub fn revalidate(address: Address, is_testnet: bool) -> Result<Address> {
    revalidate_network(
        address,
        if is_testnet {
            bitcoin::Network::Testnet
        } else {
            bitcoin::Network::Bitcoin
        },
    )
}