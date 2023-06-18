// standard
use std::default::Default;
use std::fs;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::SocketAddr;
use std::ops::Drop;
use std::path::Path;
use std::process;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::str::FromStr;
use std::string::ToString;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// internal crates
use crate::tor_crypto::*;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("failed to read control port file")]
    ControlPortFileReadFailed(#[source] std::io::Error),

    #[error("provided control port file '{0}' larger than expected ({1} bytes)")]
    ControlPortFileTooLarge(String, u64),

    #[error("failed to parse '{0}' as control port file")]
    ControlPortFileContentsInvalid(String),

    #[error("provided tor bin path '{0}' must be an absolute path")]
    TorBinPathNotAbsolute(String),

    #[error("provided data directory '{0}' must be an absolute path")]
    TorDataDirectoryPathNotAbsolute(String),

    #[error("failed to create data directory")]
    DataDirectoryCreationFailed(#[source] std::io::Error),

    #[error("file exists in provided data directory path '{0}'")]
    DataDirectoryPathExistsAsFile(String),

    #[error("failed to create default_torrc file")]
    DefaultTorrcFileCreationFailed(#[source] std::io::Error),

    #[error("failed to write default_torrc file")]
    DefaultTorrcFileWriteFailed(#[source] std::io::Error),

    #[error("failed to create torrc file")]
    TorrcFileCreationFailed(#[source] std::io::Error),

    #[error("failed to remove control_port file")]
    ControlPortFileDeleteFailed(#[source] std::io::Error),

    #[error("failed to start tor process")]
    TorProcessStartFailed(#[source] std::io::Error),

    #[error("failed to read control addr from control_file '{0}'")]
    ControlPortFileMissing(String),

    #[error("unable to take tor process stdout")]
    TorProcessStdoutTakeFailed(),

    #[error("failed to spawn tor process stdout read thread")]
    StdoutReadThreadSpawnFailed(#[source] std::io::Error),
}

fn read_control_port_file(control_port_file: &Path) -> Result<SocketAddr, Error> {
    // open file
    let mut file = File::open(control_port_file).map_err(Error::ControlPortFileReadFailed)?;

    // bail if the file is larger than expected
    let metadata = file.metadata().map_err(Error::ControlPortFileReadFailed)?;
    if metadata.len() >= 1024 {
        return Err(Error::ControlPortFileTooLarge(
            format!("{}", control_port_file.display()),
            metadata.len(),
        ));
    }

    // read contents to string
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(Error::ControlPortFileReadFailed)?;

    if contents.starts_with("PORT=") {
        let addr_string = &contents.trim_end()["PORT=".len()..];
        if let Ok(addr) = SocketAddr::from_str(addr_string) {
            return Ok(addr);
        }
    }
    Err(Error::ControlPortFileContentsInvalid(format!(
        "{}",
        control_port_file.display()
    )))
}

// Encapsulates the tor daemon process
pub(crate) struct TorProcess {
    control_addr: SocketAddr,
    process: Child,
    password: String,
    // stdout data
    stdout_lines: Arc<Mutex<Vec<String>>>,
}

impl TorProcess {
    pub fn get_control_addr(&self) -> &SocketAddr {
        &self.control_addr
    }

    pub fn get_password(&self) -> &String {
        &self.password
    }

    pub fn new(tor_bin_path: &Path, data_directory: &Path) -> Result<TorProcess, Error> {
        if tor_bin_path.is_relative() {
            return Err(Error::TorBinPathNotAbsolute(format!(
                "{}",
                tor_bin_path.display()
            )));
        }
        if data_directory.is_relative() {
            return Err(Error::TorDataDirectoryPathNotAbsolute(format!(
                "{}",
                data_directory.display()
            )));
        }

        // create data directory if it doesn't exist
        if !data_directory.exists() {
            fs::create_dir_all(data_directory).map_err(Error::DataDirectoryCreationFailed)?;
        } else if data_directory.is_file() {
            return Err(Error::DataDirectoryPathExistsAsFile(format!(
                "{}",
                data_directory.display()
            )));
        }

        // construct paths to torrc files
        let default_torrc = data_directory.join("default_torrc");
        let torrc = data_directory.join("torrc");
        let control_port_file = data_directory.join("control_port");

        // TODO: should we nuke the existing torrc between runs? Do we want
        // users setting custom nonsense in there?
        // construct default torrc
        //  - daemon determines socks port and only allows clients to connect to onion services
        //  - minimize writes to disk
        //  - start with network disabled by default
        if !default_torrc.exists() {
            const DEFAULT_TORRC_CONTENT: &str = "SocksPort auto OnionTrafficOnly\n\
            AvoidDiskWrites 1\n\
            DisableNetwork 1\n\n";

            let mut default_torrc_file =
                File::create(&default_torrc).map_err(Error::DefaultTorrcFileCreationFailed)?;
            default_torrc_file
                .write_all(DEFAULT_TORRC_CONTENT.as_bytes())
                .map_err(Error::DefaultTorrcFileWriteFailed)?;
        }

        // create empty torrc for user
        if !torrc.exists() {
            let _ = File::create(&torrc).map_err(Error::TorrcFileCreationFailed)?;
        }

        // remove any existing control_port_file
        if control_port_file.exists() {
            fs::remove_file(&control_port_file).map_err(Error::ControlPortFileDeleteFailed)?;
        }

        const CONTROL_PORT_PASSWORD_LENGTH: usize = 32usize;
        let password = generate_password(CONTROL_PORT_PASSWORD_LENGTH);
        let password_hash = hash_tor_password(&password);

        let mut process = Command::new(tor_bin_path.as_os_str())
            .stdout(Stdio::piped())
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            // point to our above written torrc file
            .arg("--defaults-torrc")
            .arg(default_torrc)
            // location of torrc
            .arg("--torrc-file")
            .arg(torrc)
            // root data directory
            .arg("DataDirectory")
            .arg(data_directory)
            // daemon will assign us a port, and we will
            // read it from the control port file
            .arg("ControlPort")
            .arg("auto")
            // control port file destination
            .arg("ControlPortWriteToFile")
            .arg(control_port_file.clone())
            // use password authentication to prevent other apps
            // from modifying our daemon's settings
            .arg("HashedControlPassword")
            .arg(password_hash)
            // tor process will shut down after this process shuts down
            // to avoid orphaned tor daemon
            .arg("__OwningControllerProcess")
            .arg(process::id().to_string())
            .spawn()
            .map_err(Error::TorProcessStartFailed)?;

        let mut control_addr = None;
        let start = Instant::now();

        // try and read the control port from the control port file
        // or abort after 5 seconds
        // TODO: make this timeout configurable?
        while control_addr.is_none() && start.elapsed() < Duration::from_secs(5) {
            if control_port_file.exists() {
                control_addr = Some(read_control_port_file(control_port_file.as_path())?);
                fs::remove_file(&control_port_file).map_err(Error::ControlPortFileDeleteFailed)?;
            }
        }

        let control_addr = match control_addr {
            Some(control_addr) => control_addr,
            None => {
                return Err(Error::ControlPortFileMissing(format!(
                    "{}",
                    control_port_file.display()
                )))
            }
        };

        let stdout_lines: Arc<Mutex<Vec<String>>> = Default::default();

        {
            let stdout_lines = Arc::downgrade(&stdout_lines);
            let stdout = BufReader::new(match process.stdout.take() {
                Some(stdout) => stdout,
                None => return Err(Error::TorProcessStdoutTakeFailed()),
            });

            std::thread::Builder::new()
                .name("tor_stdout_reader".to_string())
                .spawn(move || {
                    TorProcess::read_stdout_task(&stdout_lines, stdout);
                })
                .map_err(Error::StdoutReadThreadSpawnFailed)?;
        }

        Ok(TorProcess {
            control_addr,
            process,
            password,
            stdout_lines,
        })
    }

    fn read_stdout_task(
        stdout_lines: &std::sync::Weak<Mutex<Vec<String>>>,
        mut stdout: BufReader<ChildStdout>,
    ) {
        while let Some(stdout_lines) = stdout_lines.upgrade() {
            let mut line = String::default();
            // read line
            if stdout.read_line(&mut line).is_ok() {
                // remove trailing '\n'
                line.pop();
                // then acquire the lock on the line buffer
                let mut stdout_lines = match stdout_lines.lock() {
                    Ok(stdout_lines) => stdout_lines,
                    Err(_) => unreachable!(),
                };
                stdout_lines.push(line);
            }
        }
    }

    pub fn wait_log_lines(&mut self) -> Vec<String> {
        let mut lines = match self.stdout_lines.lock() {
            Ok(lines) => lines,
            Err(_) => unreachable!(),
        };
        std::mem::take(&mut lines)
    }
}

impl Drop for TorProcess {
    fn drop(&mut self) {
        let _ = self.process.kill();
    }
}
