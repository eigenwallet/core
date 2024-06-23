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

// extern crates
use data_encoding::HEXUPPER;
use rand::rngs::OsRng;
use rand::RngCore;
use sha1::{Digest, Sha1};

// internal crates
use crate::tor_crypto::generate_password;

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

    #[error("failed to start legacy tor process")]
    LegacyTorProcessStartFailed(#[source] std::io::Error),

    #[error("failed to read control addr from control_file '{0}'")]
    ControlPortFileMissing(String),

    #[error("unable to take legacy tor process stdout")]
    LegacyTorProcessStdoutTakeFailed(),

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
pub(crate) struct LegacyTorProcess {
    control_addr: SocketAddr,
    process: Child,
    password: String,
    // stdout data
    stdout_lines: Arc<Mutex<Vec<String>>>,
}

impl LegacyTorProcess {
    const S2K_RFC2440_SPECIFIER_LEN: usize = 9;

    fn hash_tor_password_with_salt(
        salt: &[u8; Self::S2K_RFC2440_SPECIFIER_LEN],
        password: &str,
    ) -> String {
        assert_eq!(salt[Self::S2K_RFC2440_SPECIFIER_LEN - 1], 0x60);

        // tor-specific rfc 2440 constants
        const EXPBIAS: u8 = 6u8;
        const C: u8 = 0x60; // salt[S2K_RFC2440_SPECIFIER_LEN - 1]
        const COUNT: usize = (16usize + ((C & 15u8) as usize)) << ((C >> 4) + EXPBIAS);

        // squash together our hash input
        let mut input: Vec<u8> = Default::default();
        // append salt (sans the 'C' constant')
        input.extend_from_slice(&salt[0..Self::S2K_RFC2440_SPECIFIER_LEN - 1]);
        // append password bytes
        input.extend_from_slice(password.as_bytes());

        let input = input.as_slice();
        let input_len = input.len();

        let mut sha1 = Sha1::new();
        let mut count = COUNT;
        while count > 0 {
            if count > input_len {
                sha1.update(input);
                count -= input_len;
            } else {
                sha1.update(&input[0..count]);
                break;
            }
        }

        let key = sha1.finalize();

        let mut hash = "16:".to_string();
        HEXUPPER.encode_append(salt, &mut hash);
        HEXUPPER.encode_append(&key, &mut hash);

        hash
    }

    fn hash_tor_password(password: &str) -> String {
        let mut salt = [0x00u8; Self::S2K_RFC2440_SPECIFIER_LEN];
        OsRng.fill_bytes(&mut salt);
        salt[Self::S2K_RFC2440_SPECIFIER_LEN - 1] = 0x60u8;

        Self::hash_tor_password_with_salt(&salt, password)
    }

    pub fn get_control_addr(&self) -> &SocketAddr {
        &self.control_addr
    }

    pub fn get_password(&self) -> &String {
        &self.password
    }

    pub fn new(tor_bin_path: &Path, data_directory: &Path) -> Result<LegacyTorProcess, Error> {
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
        //  - daemon determines socks port
        //  - minimize writes to disk
        //  - start with network disabled by default
        if !default_torrc.exists() {
            const DEFAULT_TORRC_CONTENT: &str = "SocksPort auto\n\
            AvoidDiskWrites 1\n\
            DisableNetwork 1\n";

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
        let password_hash = Self::hash_tor_password(&password);

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
            .map_err(Error::LegacyTorProcessStartFailed)?;

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
                None => return Err(Error::LegacyTorProcessStdoutTakeFailed()),
            });

            std::thread::Builder::new()
                .name("tor_stdout_reader".to_string())
                .spawn(move || {
                    LegacyTorProcess::read_stdout_task(&stdout_lines, stdout);
                })
                .map_err(Error::StdoutReadThreadSpawnFailed)?;
        }

        Ok(LegacyTorProcess {
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

impl Drop for LegacyTorProcess {
    fn drop(&mut self) {
        let _ = self.process.kill();
    }
}

#[test]
fn test_password_hash() -> Result<(), anyhow::Error> {
    let salt1: [u8; LegacyTorProcess::S2K_RFC2440_SPECIFIER_LEN] = [
        0xbeu8, 0x2au8, 0x25u8, 0x1du8, 0xe6u8, 0x2cu8, 0xb2u8, 0x7au8, 0x60u8,
    ];
    let hash1 = LegacyTorProcess::hash_tor_password_with_salt(&salt1, "abcdefghijklmnopqrstuvwxyz");
    assert_eq!(
        hash1,
        "16:BE2A251DE62CB27A60AC9178A937990E8ED0AB662FA82A5C7DE3EBB23A"
    );

    let salt2: [u8; LegacyTorProcess::S2K_RFC2440_SPECIFIER_LEN] = [
        0x36u8, 0x73u8, 0x0eu8, 0xefu8, 0xd1u8, 0x8cu8, 0x60u8, 0xd6u8, 0x60u8,
    ];
    let hash2 = LegacyTorProcess::hash_tor_password_with_salt(&salt2, "password");
    assert_eq!(
        hash2,
        "16:36730EEFD18C60D66052E7EA535438761C0928D316EEA56A190C99B50A"
    );

    // ensure same password is hashed to different things
    assert_ne!(
        LegacyTorProcess::hash_tor_password("password"),
        LegacyTorProcess::hash_tor_password("password")
    );

    Ok(())
}
