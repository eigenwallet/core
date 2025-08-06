// standard
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::ops::Drop;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::path::Path;
use std::sync::{Mutex, Weak};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("provided arti bin path '{0}' must be an absolute path")]
    ArtiBinPathNotAbsolute(String),

    #[error("provided data directory '{0}' must be an absolute path")]
    ArtiDataDirectoryPathNotAbsolute(String),

    #[error("failed to create data directory: {0}")]
    ArtiDataDirectoryCreationFailed(#[source] std::io::Error),

    #[error("file exists in provided data directory path '{0}'")]
    ArtiDataDirectoryPathExistsAsFile(String),

    #[error("unable to set permissions for data directory: {0}")]
    ArtiDataDirectorySetPermissionsFailed(#[source] std::io::Error),

    #[error("failed to create arti.toml file: {0}")]
    ArtiTomlFileCreationFailed(#[source] std::io::Error),

    #[error("failed to write arti.toml file: {0}")]
    ArtiTomlFileWriteFailed(#[source] std::io::Error),

    #[error("failed to create rpc.toml file: {0}")]
    RpcTomlFileCreationFailed(#[source] std::io::Error),

    #[error("failed to write rpc.toml file: {0}")]
    RpcTomlFileWriteFailed(#[source] std::io::Error),

    #[error("failed to start arti process: {0}")]
    ArtiProcessStartFailed(#[source] std::io::Error),

    #[error("unable to take arti process stdout")]
    ArtiProcessStdoutTakeFailed(),

    #[error("failed to spawn arti process stdout read thread: {0}")]
    ArtiStdoutReadThreadSpawnFailed(#[source] std::io::Error),
}

pub(crate) struct ArtiProcess {
    process: Child,
    connect_string: String,
}

impl ArtiProcess {
    pub fn new(arti_bin_path: &Path, data_directory: &Path, stdout_lines: Weak<Mutex<Vec<String>>>) -> Result<Self, Error> {
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

        // arti data directory must not be world-writable on unix platforms when using a unix domain socket endpoint
        if cfg!(unix) {
            let perms = PermissionsExt::from_mode(0o700);
            fs::set_permissions(data_directory, perms).map_err(Error::ArtiDataDirectorySetPermissionsFailed)?;
        }

        // construct paths to arti files file
        let arti_toml = data_directory.join("arti.toml").display().to_string();
        let cache_dir = data_directory.join("cache").display().to_string();
        let state_dir = data_directory.join("state").display().to_string();

        let mut arti_toml_content = format!("\
        [rpc]\n\
        enable = true\n\n\
        [rpc.listen.user-default]\n\
        enable = false\n\n\
        [rpc.listen.system-default]\n\
        enable = false\n\n\
        [storage]\n\
        cache_dir = \"{cache_dir}\"\n\
        state_dir = \"{state_dir}\"\n\n\
        [storage.keystore]\n\
        enabled = true\n\n\
        [storage.keystore.primary]\n\
        kind = \"ephemeral\"\n\n\
        [storage.permissions]\n\
        dangerously_trust_everyone = true\n\n\
        ");

        let connect_string = if cfg!(unix) {
            // use domain socket for unix
            let unix_rpc_toml_path = data_directory.join("rpc.toml").display().to_string();

            arti_toml_content.push_str(format!("\
            [rpc.listen.unix-point]\n\
            enable = true\n\
            file = \"{unix_rpc_toml_path}\"\n\n\
            ").as_str());

            let socket_path = data_directory.join("rpc.socket").display().to_string();

            let unix_rpc_toml_content = format!("\
            [connect]\n\
            socket = \"unix:{socket_path}\"\n\
            auth = \"none\"\n\
            ");

            let mut unix_rpc_toml_file =
                File::create(&unix_rpc_toml_path).map_err(Error::RpcTomlFileCreationFailed)?;
            unix_rpc_toml_file
                .write_all(unix_rpc_toml_content.as_bytes())
                .map_err(Error::RpcTomlFileWriteFailed)?;

            unix_rpc_toml_path
        } else {
            // use tcp socket everywhere else
            let tcp_rpc_toml_path = data_directory.join("rpc.toml").display().to_string();

            arti_toml_content.push_str(format!("\
            [rpc.listen.tcp-point]\n\
            enable = true\n\
            file = \"{tcp_rpc_toml_path}\"\n\n\
            ").as_str());

            let cookie_path = data_directory.join("rpc.cookie").display().to_string();

            const RPC_PORT: u16 = 18929;

            let tcp_rpc_toml_content = format!("\
            [connect]\n\
            socket = \"inet:127.0.0.1:{RPC_PORT}\"\n\
            auth = {{ cookie = {{ path = \"{cookie_path}\" }} }}\n\
            ");

            let mut tcp_rpc_toml_file =
                File::create(&tcp_rpc_toml_path).map_err(Error::RpcTomlFileCreationFailed)?;
            tcp_rpc_toml_file
                .write_all(tcp_rpc_toml_content.as_bytes())
                .map_err(Error::RpcTomlFileWriteFailed)?;

            tcp_rpc_toml_path
        };

        let mut arti_toml_file =
            File::create(&arti_toml).map_err(Error::ArtiTomlFileCreationFailed)?;
        arti_toml_file
            .write_all(arti_toml_content.as_bytes())
            .map_err(Error::ArtiTomlFileWriteFailed)?;

        let mut process = Command::new(arti_bin_path.as_os_str())
            .stdout(Stdio::piped())
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

        // spawn a task to read stdout lines and forward to list
        let stdout = BufReader::new(match process.stdout.take() {
            Some(stdout) => stdout,
            None => return Err(Error::ArtiProcessStdoutTakeFailed()),
        });
        std::thread::Builder::new()
            .name("arti_stdout_reader".to_string())
            .spawn(move || {
                ArtiProcess::read_stdout_task(&stdout_lines, stdout);
            })
            .map_err(Error::ArtiStdoutReadThreadSpawnFailed)?;

        Ok(ArtiProcess { process, connect_string })
    }

    pub fn connect_string(&self) -> &str {
        self.connect_string.as_str()
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
}

impl Drop for ArtiProcess {
    fn drop(&mut self) {
        let _ = self.process.kill();
    }
}
