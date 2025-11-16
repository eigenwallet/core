use anyhow::bail;
use anyhow::Context;
use bdk_electrum::electrum_client::HeaderNotification;
use serde::{Deserialize, Serialize};
use std::ops::Add;

/// Represent a block height, or block number, expressed in absolute block
/// count.
///
/// E.g. The transaction was included in block #655123, 655123 blocks
/// after the genesis block.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BlockHeight(u32);

impl From<BlockHeight> for u32 {
    fn from(height: BlockHeight) -> Self {
        height.0
    }
}

impl From<u32> for BlockHeight {
    fn from(height: u32) -> Self {
        Self(height)
    }
}

impl TryFrom<HeaderNotification> for BlockHeight {
    type Error = anyhow::Error;

    fn try_from(value: HeaderNotification) -> Result<Self, Self::Error> {
        Ok(Self(
            value
                .height
                .try_into()
                .context("Failed to fit usize into u32")?,
        ))
    }
}

impl Add<u32> for BlockHeight {
    type Output = BlockHeight;
    fn add(self, rhs: u32) -> Self::Output {
        BlockHeight(self.0 + rhs)
    }
}

pub mod bitcoin_address {
    use anyhow::{Context, Result};
    use bitcoin::{
        address::{NetworkChecked, NetworkUnchecked},
        Address,
    };
    use serde::Serialize;
    use std::str::FromStr;

    #[derive(thiserror::Error, Debug, Clone, Copy, PartialEq, Serialize)]
    #[error(
        "Invalid Bitcoin address provided, expected address on network {expected:?}  but address provided is on {actual:?}"
    )]
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
}

/// Bitcoin error codes: https://github.com/bitcoin/bitcoin/blob/97d3500601c1d28642347d014a6de1e38f53ae4e/src/rpc/protocol.h#L23
pub enum RpcErrorCode {
    /// Transaction or block was rejected by network rules. Error code -26.
    RpcVerifyRejected,
    /// Transaction or block was rejected by network rules. Error code -27.
    RpcVerifyAlreadyInChain,
    /// General error during transaction or block submission
    RpcVerifyError,
    /// Invalid address or key. Error code -5. Is throwns when a transaction is not found.
    /// See:
    /// - https://github.com/bitcoin/bitcoin/blob/ae024137bda9fe189f4e7ccf26dbaffd44cbbeb6/src/rpc/mempool.cpp#L470-L472
    /// - https://github.com/bitcoin/bitcoin/blob/ae024137bda9fe189f4e7ccf26dbaffd44cbbeb6/src/rpc/rawtransaction.cpp#L352-L368
    RpcInvalidAddressOrKey,
}

impl From<RpcErrorCode> for i64 {
    fn from(code: RpcErrorCode) -> Self {
        match code {
            RpcErrorCode::RpcVerifyError => -25,
            RpcErrorCode::RpcVerifyRejected => -26,
            RpcErrorCode::RpcVerifyAlreadyInChain => -27,
            RpcErrorCode::RpcInvalidAddressOrKey => -5,
        }
    }
}

pub fn parse_rpc_error_code(error: &anyhow::Error) -> anyhow::Result<i64> {
    // First try to extract an Electrum error from a MultiError if present
    if let Some(multi_error) = error.downcast_ref::<electrum_pool::MultiError>() {
        // Try to find the first Electrum error in the MultiError
        for single_error in multi_error.iter() {
            if let bdk_electrum::electrum_client::Error::Protocol(serde_json::Value::String(
                string,
            )) = single_error
            {
                let json = serde_json::from_str(
                    &string
                        .replace("sendrawtransaction RPC error:", "")
                        .replace("daemon error:", ""),
                )?;

                let json_map = match json {
                    serde_json::Value::Object(map) => map,
                    _ => continue, // Try next error if this one isn't a JSON object
                };

                let error_code_value = match json_map.get("code") {
                    Some(val) => val,
                    None => continue, // Try next error if no error code field
                };

                let error_code_number = match error_code_value {
                    serde_json::Value::Number(num) => num,
                    _ => continue, // Try next error if error code isn't a number
                };

                if let Some(int) = error_code_number.as_i64() {
                    return Ok(int);
                }
            }
        }
        // If we couldn't extract an RPC error code from any error in the MultiError
        bail!(
            "Error is of incorrect variant. We expected an Electrum error, but got: {}",
            error
        );
    }

    // Original logic for direct Electrum errors
    let string = match error.downcast_ref::<bdk_electrum::electrum_client::Error>() {
        Some(bdk_electrum::electrum_client::Error::Protocol(serde_json::Value::String(string))) => {
            string
        }
        _ => bail!(
            "Error is of incorrect variant. We expected an Electrum error, but got: {}",
            error
        ),
    };

    let json = serde_json::from_str(
        &string
            .replace("sendrawtransaction RPC error:", "")
            .replace("daemon error:", ""),
    )?;

    let json_map = match json {
        serde_json::Value::Object(map) => map,
        _ => bail!("Json error is not json object "),
    };

    let error_code_value = match json_map.get("code") {
        Some(val) => val,
        None => bail!("No error code field"),
    };

    let error_code_number = match error_code_value {
        serde_json::Value::Number(num) => num,
        _ => bail!("Error code is not a number"),
    };

    if let Some(int) = error_code_number.as_i64() {
        Ok(int)
    } else {
        bail!("Error code is not an unsigned integer")
    }
}
