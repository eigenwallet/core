use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// This is the default location for the overall config-dir specific by system
// Linux: /home/<user>/.config/xmr-btc-swap/
// OSX: /Users/<user>/Library/Application Support/xmr-btc-swap/
pub fn system_config_dir() -> Result<PathBuf> {
    dirs::config_dir()
        .map(|cd| cd.join("xmr-btc-swap"))
        .context("Could not generate default system configuration dir path")
}

/// This is the default location for the overall data-dir specific by system
// Linux: /home/<user>/.local/share/xmr-btc-swap/
// OSX: /Users/<user>/Library/Application Support/xmr-btc-swap/
pub fn system_data_dir() -> Result<PathBuf> {
    dirs::data_dir()
        .map(|cd| cd.join("xmr-btc-swap"))
        .context("Could not generate default system data-dir dir path")
}

pub fn system_data_dir_eigenwallet(testnet: bool) -> Result<PathBuf> {
    let application_directory = match testnet {
        true => "eigenwallet-testnet",
        false => "eigenwallet",
    };

    dirs::data_dir()
        .map(|cd| cd.join(application_directory))
        .context("Could not generate default system data-dir dir path")
}

pub fn ensure_directory_exists(file: &Path) -> Result<(), std::io::Error> {
    if let Some(path) = file.parent() {
        if !path.exists() {
            tracing::info!(
                directory = %file.display(),
                "Parent directory does not exist, creating recursively",
            );
            return std::fs::create_dir_all(path);
        }
    }
    Ok(())
}
