//! The eigenwallet startup flow: opening or creating the Monero wallet the
//! app runs on, driven by an explicit state machine ([`SetupFlow`]).
//!
//! Every state that needs user input blocks on a Tauri approval; the GUI is a
//! renderer of whichever request is currently pending.

use super::*;
use crate::cli::api::tauri_bindings::{SeedBackupDetails, SeedChoice, SeedSelectionDetails};

// Legacy mode uses this Monero monitoring wallet and the seed.pem-derived
// Bitcoin wallet in the same CLI data directory.
const LEGACY_MONITORING_WALLET_NAME: &str = "swap-tool-blockchain-monitoring-wallet";

/// The wallet-setup flow as an explicit state machine. Transitions:
///
/// ```text
/// ChooseSeed ──► OpenWallet ──► BackupSeed ──► Finish
///     ▲              │ (create)      (fresh wallet)
///     └──────────────┘ (failure or password rejection)
/// ```
enum SetupFlow {
    /// Ask the user how to open a wallet.
    ChooseSeed,
    /// Attempt to open or create the wallet for the user's choice.
    OpenWallet(SeedChoice),
    /// A freshly generated wallet: show the seed until the user confirms
    /// having backed it up.
    BackupSeed(monero_sys::WalletHandle),
    /// The wallet is open; extract the swap seed and finish.
    Finish(monero_sys::WalletHandle),
}

/// Opens or creates a Monero wallet by driving [`SetupFlow`].
///
/// The user can:
/// - Create a new wallet with a random seed (the seed must be backed up
///   before startup continues).
/// - Recover a wallet from a given seed phrase.
/// - Open an existing wallet file (with password verification).
///
/// A wallet that fails to open or create puts the user back into the
/// chooser; hard errors (e.g. the approval UI going away) propagate.
pub(super) async fn open_monero_wallet(
    tauri_handle: Option<TauriHandle>,
    eigenwallet_data_dir: &Path,
    legacy_data_dir: &PathBuf,
    env_config: EnvConfig,
    daemon: &monero_sys::Daemon,
    seed_choice: Option<SeedChoice>,
    database: &monero_sys::Database,
) -> Result<(monero_sys::WalletHandle, Seed), Error> {
    // Without a seed choice there is no UI driving the flow: the CLI uses
    // the legacy wallet to monitor the blockchain.
    let Some(seed_choice) = seed_choice else {
        let wallet =
            request_and_open_monero_wallet_legacy(legacy_data_dir, env_config, daemon).await?;
        let seed = Seed::from_file_or_generate(legacy_data_dir)
            .await
            .context("Failed to read legacy seed from file")?;

        return Ok((wallet, seed));
    };

    let mut flow = SetupFlow::OpenWallet(seed_choice);

    loop {
        flow = match flow {
            SetupFlow::ChooseSeed => SetupFlow::OpenWallet(
                request_seed_choice(
                    tauri_handle.clone().unwrap(),
                    database,
                    eigenwallet_data_dir,
                )
                .await?,
            ),
            SetupFlow::OpenWallet(choice) => {
                let _monero_progress_handle = tauri_handle
                    .new_background_process_with_initial_progress(
                        TauriBackgroundProgress::OpeningMoneroWallet,
                        (),
                    );

                let opened: Result<SetupFlow> = match choice {
                    SeedChoice::RandomSeed {
                        password,
                        name,
                        directory,
                    } => {
                        async {
                            let wallet_path = new_wallet_path(&directory, &name)
                                .context("Failed to determine path for new wallet")?;

                            let wallet = monero::Wallet::open_or_create_with_password(
                                wallet_path.display().to_string(),
                                if password.is_empty() {
                                    None
                                } else {
                                    Some(password)
                                },
                                daemon.clone(),
                                env_config.monero_network,
                                true,
                            )
                            .await
                            .context("Failed to create wallet from random seed")?;

                            Ok(SetupFlow::BackupSeed(wallet))
                        }
                        .await
                    }
                    SeedChoice::FromSeed {
                        seed: mnemonic,
                        restore_height,
                        password,
                        name,
                        directory,
                    } => {
                        async {
                            let wallet_path = new_wallet_path(&directory, &name)
                                .context("Failed to determine path for new wallet")?;

                            let wallet = monero::Wallet::open_or_create_from_seed_with_password(
                                wallet_path.display().to_string(),
                                mnemonic,
                                if password.is_empty() {
                                    None
                                } else {
                                    Some(password)
                                },
                                env_config.monero_network,
                                restore_height.into(),
                                true,
                                daemon.clone(),
                            )
                            .await
                            .context("Failed to create wallet from provided seed")?;

                            Ok(SetupFlow::Finish(wallet))
                        }
                        .await
                    }
                    SeedChoice::FromWalletPath { wallet_path } => {
                        if is_legacy_wallet_path(&wallet_path, legacy_data_dir) {
                            let wallet = request_and_open_monero_wallet_legacy(
                                legacy_data_dir,
                                env_config,
                                daemon,
                            )
                            .await?;
                            let seed = Seed::from_file_or_generate(legacy_data_dir)
                                .await
                                .context("Failed to read legacy seed from file")?;

                            return Ok((wallet, seed));
                        }

                        // Helper function to verify password
                        let verify_password = |password: String| -> Result<bool> {
                            monero_sys::WalletHandle::verify_wallet_password(
                                wallet_path.clone(),
                                password,
                            )
                            .map_err(|e| anyhow::anyhow!("Failed to verify wallet password: {}", e))
                        };

                        // Request and verify password before opening wallet
                        let wallet_password: Option<String> = {
                            const WALLET_EMPTY_PASSWORD: &str = "";

                            // First try empty password
                            if verify_password(WALLET_EMPTY_PASSWORD.to_string())? {
                                Some(WALLET_EMPTY_PASSWORD.to_string())
                            } else {
                                // If empty password fails, ask user for password
                                loop {
                                    // Request password from user
                                    let password = tauri_handle
                                        .request_password(wallet_path.clone())
                                        .await
                                        .inspect_err(|e| {
                                            tracing::error!(
                                                "Failed to get password from user: {}",
                                                e
                                            );
                                        })
                                        .ok();

                                    // If the user rejects the password request (presses cancel)
                                    // We prompt him to select a wallet again
                                    let password = match password {
                                        Some(password) => password,
                                        None => break None,
                                    };

                                    // Verify the password using the helper function
                                    match verify_password(password.clone()) {
                                        Ok(true) => {
                                            break Some(password);
                                        }
                                        Ok(false) => {
                                            // Continue loop to request password again
                                            continue;
                                        }
                                        Err(e) => {
                                            return Err(e);
                                        }
                                    }
                                }
                            }
                        };

                        match wallet_password {
                            // The user rejected the password request: back to
                            // the chooser.
                            None => Ok(SetupFlow::ChooseSeed),
                            // Open existing wallet with verified password
                            Some(password) => monero::Wallet::open_or_create_with_password(
                                wallet_path.clone(),
                                password,
                                daemon.clone(),
                                env_config.monero_network,
                                true,
                            )
                            .await
                            .context("Failed to open wallet from provided path")
                            .map(SetupFlow::Finish),
                        }
                    }
                    SeedChoice::Legacy => {
                        let wallet = request_and_open_monero_wallet_legacy(
                            legacy_data_dir,
                            env_config,
                            daemon,
                        )
                        .await?;
                        let seed = Seed::from_file_or_generate(legacy_data_dir)
                            .await
                            .context("Failed to read legacy seed from file")?;

                        return Ok((wallet, seed));
                    }
                };

                // A failed create/open (e.g. the wallet name is already taken)
                // must not abort startup: put the user back into the chooser
                // instead.
                match opened {
                    Ok(next) => next,
                    Err(error) => {
                        tracing::error!(
                            ?error,
                            "Failed to open or create wallet, asking user to choose again"
                        );
                        SetupFlow::ChooseSeed
                    }
                }
            }
            SetupFlow::BackupSeed(wallet) => {
                // Block until the user confirms having recorded the seed. If
                // the backup approval cannot complete, keep going: the wallet
                // already exists and the seed stays viewable from the wallet
                // page.
                let backup = async {
                    let details = SeedBackupDetails {
                        seed: wallet.seed().await?,
                        restore_height: wallet.get_restore_height().await?,
                    };
                    tauri_handle.request_seed_backup(details).await
                }
                .await;

                match backup {
                    Ok(true) => {}
                    Ok(false) => {
                        tracing::warn!("Seed backup rejected by user, continuing startup")
                    }
                    Err(error) => {
                        tracing::warn!(
                            ?error,
                            "Seed backup could not be confirmed, continuing startup"
                        )
                    }
                }

                SetupFlow::Finish(wallet)
            }
            SetupFlow::Finish(wallet) => {
                let seed = Seed::from_monero_wallet(&wallet)
                    .await
                    .context("Failed to extract seed from wallet")?;

                return Ok((wallet, seed));
            }
        };
    }
}

/// Requests the user to select a seed choice from a list of recent wallets
pub(super) async fn request_seed_choice(
    tauri_handle: TauriHandle,
    database: &monero_sys::Database,
    eigenwallet_data_dir: &Path,
) -> Result<SeedChoice> {
    let recent_wallets = database.get_recent_wallets(5).await?;

    tauri_handle
        .request_seed_selection(SeedSelectionDetails {
            recent_wallets: recent_wallets.into_iter().map(|w| w.wallet_path).collect(),
            default_wallet_directory: default_wallet_directory(eigenwallet_data_dir)
                .display()
                .to_string(),
        })
        .await
}

/// Default directory new wallet files are stored in.
fn default_wallet_directory(eigenwallet_data_dir: &Path) -> PathBuf {
    eigenwallet_data_dir.join("wallets")
}

/// Builds the path for a freshly created wallet. The name must be a single
/// path component so it cannot escape the chosen directory, and the wallet
/// must not already exist (creating must never silently open an existing
/// wallet).
fn new_wallet_path(directory: &str, name: &str) -> Result<PathBuf> {
    if name.trim().is_empty() {
        anyhow::bail!("Wallet name must not be empty");
    }

    let is_single_component = Path::new(name)
        .components()
        .eq(std::iter::once(std::path::Component::Normal(name.as_ref())));
    if !is_single_component {
        anyhow::bail!("Wallet name must be a single file name, got {name:?}");
    }

    if directory.trim().is_empty() || !Path::new(directory).is_absolute() {
        anyhow::bail!("Wallet directory must be an absolute path, got {directory:?}");
    }

    let wallet_path = PathBuf::from(directory).join(name);
    // Monero stores a wallet as `<name>` plus `<name>.keys`; `with_extension`
    // would truncate a name containing dots.
    let keys_path = PathBuf::from(directory).join(format!("{name}.keys"));

    if wallet_path.exists() || keys_path.exists() {
        anyhow::bail!("A wallet named {name:?} already exists in {directory:?}");
    }

    swap_fs::ensure_directory_exists(&wallet_path).context("Failed to create wallet directory")?;

    Ok(wallet_path)
}

fn legacy_wallet_path(data_dir: &Path) -> PathBuf {
    data_dir.join(LEGACY_MONITORING_WALLET_NAME)
}

fn is_legacy_wallet_path(wallet_path: &str, legacy_data_dir: &Path) -> bool {
    Path::new(wallet_path) == legacy_wallet_path(legacy_data_dir)
}

pub(super) async fn request_and_open_monero_wallet_legacy(
    data_dir: &PathBuf,
    env_config: EnvConfig,
    daemon: &monero_sys::Daemon,
) -> Result<monero_sys::WalletHandle, Error> {
    let wallet_path = legacy_wallet_path(data_dir);

    let wallet = monero::Wallet::open_or_create(
        wallet_path.display().to_string(),
        daemon.clone(),
        env_config.monero_network,
        true,
    )
    .await
    .context("Failed to create wallet")?;

    Ok(wallet)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin_wallet::BitcoinWalletSeed;

    #[test]
    fn detects_legacy_monitoring_wallet_path() {
        let legacy_data_dir = PathBuf::from("/tmp/eigenwallet-mainnet");
        let wallet_path = legacy_wallet_path(&legacy_data_dir);

        assert!(is_legacy_wallet_path(
            wallet_path.to_str().unwrap(),
            &legacy_data_dir,
        ));
    }

    #[test]
    fn does_not_treat_other_wallet_files_as_legacy() {
        let legacy_data_dir = PathBuf::from("/tmp/eigenwallet-mainnet");
        let wallet_path = "/tmp/eigenwallet/wallets/wallet_123";

        assert!(!is_legacy_wallet_path(wallet_path, &legacy_data_dir));
    }

    #[tokio::test]
    async fn legacy_seed_file_keeps_bitcoin_key_stable() {
        let temp_dir = tempfile::tempdir().unwrap();

        let legacy_seed = Seed::from_file_or_generate(temp_dir.path()).await.unwrap();
        let legacy_key = legacy_seed
            .derive_extended_private_key(bitcoin::Network::Bitcoin)
            .unwrap();

        let reread_legacy_seed = Seed::from_file_or_generate(temp_dir.path()).await.unwrap();
        let reread_legacy_key = reread_legacy_seed
            .derive_extended_private_key(bitcoin::Network::Bitcoin)
            .unwrap();

        let non_legacy_seed = Seed::from([0; crate::seed::SEED_LENGTH]);
        let non_legacy_key = non_legacy_seed
            .derive_extended_private_key(bitcoin::Network::Bitcoin)
            .unwrap();

        assert_eq!(legacy_key, reread_legacy_key);
        assert_ne!(legacy_key, non_legacy_key);
    }
}
