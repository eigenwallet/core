//! a harness wallet backed by the oxide funding engine
//! ([`monero_wallet_ng::funder::Funder`]) instead of wallet2.
//!
//! same surface as [`crate::MoneroWallet`] for everything the engine
//! can do today: addresses, balances, refresh, exact-amount
//! transfers. sweep, subaddresses and check_tx_key stay on the
//! wallet2 side until the engine grows them; the monero-tests suite
//! ports over file by file behind this type.
//!
//! the scan loop runs from the moment the wallet is created; it
//! starts at the daemon's current tip, so a fresh wallet sees only
//! what arrives after its creation, exactly like a fresh wallet2.

use std::collections::HashMap;

use anyhow::{Result, bail};
use monero_daemon_rpc::MoneroDaemon;
use monero_oxide_ext::PrivateKey;
use monero_simple_request_rpc::SimpleRequestTransport;
use monero_sys::TxReceipt;
use monero_wallet_ng::funder::Funder;
use monero_wallet_ng::util::public_key;
use rand::rngs::OsRng;
use zeroize::Zeroizing;

use monero_address::MoneroAddress;
use monero_oxide_wallet::address::{AddressType, Network};
use monero_oxide_wallet::ed25519::Scalar;
use monero_oxide_wallet::interface::ProvidesBlockchainMeta;
use monero_wallet_ng::rpc::{ProvidesTransactionStatus, TransactionStatus};

/// a harness wallet powered by the oxide engine.
pub struct FunderWallet {
    name: String,
    daemon: MoneroDaemon<SimpleRequestTransport>,
    funder: Funder<MoneroDaemon<SimpleRequestTransport>>,
    address: MoneroAddress,
}

impl std::fmt::Debug for FunderWallet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FunderWallet {{ name: {} }}", self.name)
    }
}

impl FunderWallet {
    /// create a wallet with fresh random keys, watching from the
    /// daemon's current tip.
    pub async fn new(name: String, daemon: MoneroDaemon<SimpleRequestTransport>) -> Result<Self> {
        let spend_key = Zeroizing::new(Scalar::random(&mut OsRng));
        let view_key = Zeroizing::new(Scalar::random(&mut OsRng));
        Self::new_with_keys(name, daemon, spend_key, view_key).await
    }

    /// create a wallet with known keys, watching from the daemon's
    /// current tip.
    pub async fn new_with_keys(
        name: String,
        daemon: MoneroDaemon<SimpleRequestTransport>,
        spend_key: Zeroizing<Scalar>,
        view_key: Zeroizing<Scalar>,
    ) -> Result<Self> {
        let address = MoneroAddress::new(
            Network::Mainnet,
            AddressType::Legacy,
            public_key(&spend_key),
            public_key(&view_key),
        );

        let restore_height = daemon.latest_block_number().await?;
        let funder = Funder::new(daemon.clone(), spend_key, view_key, restore_height)?;

        Ok(Self {
            name,
            daemon,
            funder,
            address,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub async fn address(&self) -> Result<MoneroAddress> {
        Ok(self.address)
    }

    pub async fn balance(&self) -> Result<u64> {
        Ok(self.funder.balance().await?)
    }

    pub async fn unlocked_balance(&self) -> Result<u64> {
        Ok(self.funder.unlocked_balance().await?)
    }

    /// wait until the scan loop has seen every block the daemon has.
    pub async fn refresh(&self) -> Result<()> {
        Ok(self.funder.refresh().await?)
    }

    pub async fn transfer(&self, address: &MoneroAddress, amount_pico: u64) -> Result<TxReceipt> {
        self.transfer_multi(&[(*address, amount_pico)]).await
    }

    pub async fn transfer_multi(&self, destinations: &[(MoneroAddress, u64)]) -> Result<TxReceipt> {
        validate_destinations(destinations)?;

        let receipt_addresses: Vec<MoneroAddress> =
            destinations.iter().map(|(address, _)| *address).collect();

        let transfer = self
            .funder
            .fund(destinations.to_vec(), self.address)
            .await?;

        let txid = hex::encode(transfer.transaction.hash());

        // the block the tx actually landed in, not the daemon's tip
        // afterwards: fund returns once it has one confirmation, and
        // the miner keeps moving the tip in the meantime
        let TransactionStatus::InBlock { block_height } = self
            .daemon
            .transaction_status(transfer.transaction.hash())
            .await?
        else {
            bail!("the transfer lost its confirmation before the receipt was built");
        };

        // every base-address output shares the one tx key, so the
        // receipt maps each destination to it. one entry per
        // DISTINCT address: a repeated destination collapses
        let mut tx_keys = HashMap::new();
        for address in receipt_addresses {
            tx_keys.insert(
                address.to_string(),
                PrivateKey {
                    scalar: *transfer.tx_key,
                },
            );
        }

        Ok(TxReceipt {
            txid,
            tx_keys,
            height: block_height,
        })
    }
}

/// the engine wallet's destination rules:
///
/// - at least one destination
/// - base addresses only: a subaddress output carries one of oxide's
///   additional keys i do not track, so the receipt's tx keys would
///   be a lie
/// - no repeated addresses: the receipt maps one key per distinct
///   address, so a repeat would silently collapse. merge the amounts
///   into one destination instead
fn validate_destinations(destinations: &[(MoneroAddress, u64)]) -> Result<()> {
    if destinations.is_empty() {
        bail!("no destinations given");
    }

    if let Some(address) = destinations
        .iter()
        .map(|(address, _)| address)
        .find(|address| address.is_subaddress())
    {
        bail!(
            "subaddress destination {} is not supported by the engine wallet",
            address
        );
    }

    let mut seen = std::collections::HashSet::new();
    if let Some(address) = destinations
        .iter()
        .map(|(address, _)| address)
        .find(|address| !seen.insert(address.to_string()))
    {
        bail!(
            "duplicate destination {}: merge the amounts into one destination",
            address
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_address(byte: u8) -> MoneroAddress {
        let spend = Zeroizing::new(Scalar::random(&mut rand::rngs::OsRng));
        let view = Zeroizing::new(Scalar::random(&mut rand::rngs::OsRng));
        let _ = byte;
        MoneroAddress::new(
            Network::Mainnet,
            AddressType::Legacy,
            public_key(&spend),
            public_key(&view),
        )
    }

    #[test]
    fn accepts_distinct_base_addresses() {
        let destinations = vec![(base_address(1), 1), (base_address(2), 2)];
        assert!(validate_destinations(&destinations).is_ok());
    }

    #[test]
    fn rejects_no_destinations() {
        assert!(validate_destinations(&[]).is_err());
    }

    #[test]
    fn rejects_duplicate_destinations() {
        let address = base_address(3);
        let destinations = vec![(address, 1), (address, 2)];
        let error = validate_destinations(&destinations)
            .unwrap_err()
            .to_string();
        assert!(error.contains("duplicate destination"), "{error}");
    }
}
