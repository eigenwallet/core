// standard
use std::fs;
use std::fs::File;
use std::io::Write;
use std::ops::Drop;
use std::process;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::path::Path;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("not implemented")]
    NotImplemented(),

    #[error("provided arti bin path '{0}' must be an absolute path")]
    ArtiBinPathNotAbsolute(String),

    #[error("provided data directory '{0}' must be an absolute path")]
    ArtiDataDirectoryPathNotAbsolute(String),

    #[error("failed to create data directory")]
    ArtiDataDirectoryCreationFailed(#[source] std::io::Error),

    #[error("file exists in provided data directory path '{0}'")]
    ArtiDataDirectoryPathExistsAsFile(String),

    #[error("failed to create arti.toml file")]
    ArtiTomlFileCreationFailed(#[source] std::io::Error),

    #[error("failed to write arti.toml file")]
    ArtiTomlFileWriteFailed(#[source] std::io::Error),

    #[error("failed to start arti process")]
    ArtiProcessStartFailed(#[source] std::io::Error),
}

pub(crate) struct ArtiProcess {
    process: Child,
}

impl ArtiProcess {
    pub fn new(arti_bin_path: &Path, data_directory: &Path) -> Result<Self, Error> {
        // verify provided paths are absolute
        if arti_bin_path.is_relative() {
            return Err(Error::ArtiBinPathNotAbsolute(format!(
                "{}",
                arti_bin_path.display()
            )));
        }
        if data_directory.is_relative() {
            return Err(Error::ArtiDataDirectoryPathNotAbsolute(format!(
                "{}",
                data_directory.display()
            )));
        }

        // create data directory if it doesn't exist
        if !data_directory.exists() {
            fs::create_dir_all(data_directory).map_err(Error::ArtiDataDirectoryCreationFailed)?;
        } else if data_directory.is_file() {
            return Err(Error::ArtiDataDirectoryPathExistsAsFile(format!(
                "{}",
                data_directory.display()
            )));
        }

        // construct paths to arti files file
        let arti_toml = data_directory.join("arti.toml");

        // write arti.toml settings file (always overwrite)
        let cache_dir = data_directory.join("cache").display().to_string();
        let state_dir = data_directory.join("state").display().to_string();
        let rpc_listen = data_directory.join("SOCKET").display().to_string();

        let arti_toml_content = format!("\
        [storage]\n\
        cache_dir = \"{cache_dir}\"\n\
        state_dir = \"{state_dir}\"\n\
        [storage.keystore]\n\
        enabled = true\n\
        [storage.keystore.primary]\n\
        kind = \"ephemeral\"\n\
        [storage.permissions]\n\
        dangerously_trust_everyone = true\n\
        [rpc]\n
        rpc_listen = \"{rpc_listen}\"\n
        ");

        let mut arti_toml_file =
            File::create(&arti_toml).map_err(Error::ArtiTomlFileCreationFailed)?;
        arti_toml_file
            .write_all(arti_toml_content.as_bytes())
            .map_err(Error::ArtiTomlFileWriteFailed)?;

        let process = Command::new(arti_bin_path.as_os_str())
            .stdout(Stdio::inherit())
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            // set working directory to data directory
            .current_dir(data_directory)
            // proxy subcommand
            .arg("proxy")
            // point to our above written arti.toml file
            .arg("--config")
            .arg(arti_toml)
            .spawn()
            .map_err(Error::ArtiProcessStartFailed)?;


        Ok(ArtiProcess { process })
    }
}

impl Drop for ArtiProcess {
    fn drop(&mut self) {
        let _ = self.process.kill();
    }
}
