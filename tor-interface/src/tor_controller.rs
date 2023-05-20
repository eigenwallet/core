// standard
use std::cmp::Ordering;
use std::collections::VecDeque;
use std::default::Default;
use std::fs;
use std::fs::File;
use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::iter;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::ops::Drop;
use std::option::Option;
use std::path::Path;
use std::process;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::str::FromStr;
use std::string::ToString;
use std::sync::{atomic, Arc, Mutex};
use std::time::{Duration, Instant};

// extern crates
use rand::distributions::Alphanumeric;
use rand::rngs::OsRng;
use rand::Rng;
use regex::Regex;
#[cfg(test)]
use serial_test::serial;
use socks::Socks5Stream;
use url::Host;

// internal crates
use crate::error::ControlStreamError;
use crate::error::ControlStreamError::*;
use crate::error::TorControllerError;
use crate::error::TorControllerError::*;
use crate::error::TorManagerError;
use crate::error::TorManagerError::*;
use crate::error::TorProcessError;
use crate::error::TorProcessError::*;
use crate::error::TorVersionError;
use crate::error::TorVersionError::*;
use crate::tor_crypto::*;
// get the name of our tor executable
pub const fn tor_exe_name() -> &'static str {
    if cfg!(windows) {
        "tor.exe"
    } else {
        "tor"
    }
}

// securely generate password using OsRng
fn generate_password(length: usize) -> String {
    let password: String = iter::repeat(())
        .map(|()| OsRng.sample(Alphanumeric))
        .map(char::from)
        .take(length)
        .collect();

    password
}

fn read_control_port_file(control_port_file: &Path) -> Result<SocketAddr, TorProcessError> {
    // open file
    let mut file = File::open(control_port_file).map_err(ControlPortFileReadFailed)?;

    // bail if the file is larger than expected
    let metadata = file.metadata().map_err(ControlPortFileReadFailed)?;
    if metadata.len() >= 1024 {
        return Err(ControlPortFileTooLarge(
            format!("{}", control_port_file.display()),
            metadata.len(),
        ));
    }

    // read contents to string
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(ControlPortFileReadFailed)?;

    if contents.starts_with("PORT=") {
        let addr_string = &contents.trim_end()["PORT=".len()..];
        if let Ok(addr) = SocketAddr::from_str(addr_string) {
            return Ok(addr);
        }
    }
    Err(ControlPortFileContentsInvalid(format!(
        "{}",
        control_port_file.display()
    )))
}

// Encapsulates the tor daemon process
struct TorProcess {
    control_addr: SocketAddr,
    process: Child,
    password: String,
    // stdout data
    stdout_lines: Arc<Mutex<Vec<String>>>,
}

impl TorProcess {
    pub fn new(tor_bin_path: &Path, data_directory: &Path) -> Result<TorProcess, TorProcessError> {
        if tor_bin_path.is_relative() {
            return Err(TorBinPathNotAbsolute(format!("{}", tor_bin_path.display())));
        }
        if data_directory.is_relative() {
            return Err(TorDataDirectoryPathNotAbsolute(format!(
                "{}",
                data_directory.display()
            )));
        }

        // create data directory if it doesn't exist
        if !data_directory.exists() {
            fs::create_dir_all(data_directory).map_err(DataDirectoryCreationFailed)?;
        } else if data_directory.is_file() {
            return Err(DataDirectoryPathExistsAsFile(format!(
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
                File::create(&default_torrc).map_err(DefaultTorrcFileCreationFailed)?;
            default_torrc_file
                .write_all(DEFAULT_TORRC_CONTENT.as_bytes())
                .map_err(DefaultTorrcFileWriteFailed)?;
        }

        // create empty torrc for user
        if !torrc.exists() {
            let _ = File::create(&torrc).map_err(TorrcFileCreationFailed)?;
        }

        // remove any existing control_port_file
        if control_port_file.exists() {
            fs::remove_file(&control_port_file).map_err(ControlPortFileDeleteFailed)?;
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
            .map_err(TorProcessStartFailed)?;

        let mut control_addr = None;
        let start = Instant::now();

        // try and read the control port from the control port file
        // or abort after 5 seconds
        // TODO: make this timeout configurable?
        while control_addr.is_none() && start.elapsed() < Duration::from_secs(5) {
            if control_port_file.exists() {
                control_addr = Some(read_control_port_file(control_port_file.as_path())?);
                fs::remove_file(&control_port_file).map_err(ControlPortFileDeleteFailed)?;
            }
        }

        let control_addr = match control_addr {
            Some(control_addr) => control_addr,
            None => {
                return Err(ControlPortFileMissing(format!(
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
                None => return Err(TorProcessStdoutTakeFailed()),
            });

            std::thread::Builder::new()
                .name("tor_stdout_reader".to_string())
                .spawn(move || {
                    TorProcess::read_stdout_task(&stdout_lines, stdout);
                })
                .map_err(StdoutReadThreadSpawnFailed)?;
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

    fn wait_log_lines(&mut self) -> Vec<String> {
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

pub struct ControlStream {
    stream: TcpStream,
    closed_by_remote: bool,
    pending_data: Vec<u8>,
    pending_lines: VecDeque<String>,
    pending_reply: Vec<String>,
    reading_multiline_value: bool,
    // regexes used to parse control port responses
    single_line_data: Regex,
    multi_line_data: Regex,
    end_reply_line: Regex,
}

type StatusCode = u32;
struct Reply {
    status_code: StatusCode,
    reply_lines: Vec<String>,
}

impl ControlStream {
    pub fn new(
        addr: &SocketAddr,
        read_timeout: Duration,
    ) -> Result<ControlStream, ControlStreamError> {
        if read_timeout.is_zero() {
            return Err(ReadTimeoutZero());
        }

        let stream = TcpStream::connect(addr).map_err(CreationFailed)?;
        stream
            .set_read_timeout(Some(read_timeout))
            .map_err(ConfigurationFailed)?;

        // pre-allocate a kilobyte for the read buffer
        const READ_BUFFER_SIZE: usize = 1024;
        let pending_data = Vec::with_capacity(READ_BUFFER_SIZE);

        let single_line_data =
            Regex::new(r"^\d\d\d-.*").map_err(ControlStreamError::ParsingRegexCreationFailed)?;
        let multi_line_data =
            Regex::new(r"^\d\d\d+.*").map_err(ControlStreamError::ParsingRegexCreationFailed)?;
        let end_reply_line =
            Regex::new(r"^\d\d\d .*").map_err(ControlStreamError::ParsingRegexCreationFailed)?;

        Ok(ControlStream {
            stream,
            closed_by_remote: false,
            pending_data,
            pending_lines: Default::default(),
            pending_reply: Default::default(),
            reading_multiline_value: false,
            // regex
            single_line_data,
            multi_line_data,
            end_reply_line,
        })
    }

    #[cfg(test)]
    fn closed_by_remote(&mut self) -> bool {
        self.closed_by_remote
    }

    fn read_line(&mut self) -> Result<Option<String>, ControlStreamError> {
        // read pending bytes from stream until we have a line to return
        while self.pending_lines.is_empty() {
            let byte_count = self.pending_data.len();
            match self.stream.read_to_end(&mut self.pending_data) {
                Err(err) => {
                    if err.kind() == ErrorKind::WouldBlock || err.kind() == ErrorKind::TimedOut {
                        if byte_count == self.pending_data.len() {
                            return Ok(None);
                        }
                    } else {
                        return Err(ControlStreamError::ReadFailed(err));
                    }
                }
                Ok(0usize) => {
                    self.closed_by_remote = true;
                    return Err(ControlStreamError::ClosedByRemote());
                }
                Ok(_count) => (),
            }

            // split our read buffer into individual lines
            let mut begin = 0;
            for index in 1..self.pending_data.len() {
                if self.pending_data[index - 1] == b'\r' && self.pending_data[index] == b'\n' {
                    let end = index - 1;
                    // view into byte vec of just the found line
                    let line_view: &[u8] = &self.pending_data[begin..end];
                    // convert to string
                    let line_string = std::str::from_utf8(line_view).map_err(InvalidResponse)?;

                    // save in pending list
                    self.pending_lines.push_back(line_string.to_string());
                    // update begin (and skip over \r\n)
                    begin = end + 2;
                }
            }
            // leave any leftover bytes in the buffer for the next call
            self.pending_data.drain(0..begin);
        }

        Ok(self.pending_lines.pop_front())
    }

    fn read_reply(&mut self) -> Result<Option<Reply>, ControlStreamError> {
        loop {
            let current_line = match self.read_line()? {
                Some(line) => line,
                None => return Ok(None),
            };

            // make sure the status code matches (if we are not in the
            // middle of a multi-line read
            if let Some(first_line) = self.pending_reply.first() {
                if !self.reading_multiline_value {
                    let first_status_code = &first_line[0..3];
                    let current_status_code = &current_line[0..3];
                    if first_status_code != current_status_code {
                        return Err(ReplyParseFailed(format!(
                            "mismatched status codes, {} != {}",
                            first_status_code, current_status_code
                        )));
                    }
                }
            }

            // end of a response
            if self.end_reply_line.is_match(&current_line) {
                if self.reading_multiline_value {
                    return Err(ReplyParseFailed(
                        "found multi-line end reply but not reading a multi-line reply".to_string(),
                    ));
                }
                self.pending_reply.push(current_line);
                break;
            // single line data from getinfo and friends
            } else if self.single_line_data.is_match(&current_line) {
                if self.reading_multiline_value {
                    return Err(ReplyParseFailed(
                        "found single-line reply but still reading a multi-line reply".to_string(),
                    ));
                }
                self.pending_reply.push(current_line);
            // begin of multiline data from getinfo and friends
            } else if self.multi_line_data.is_match(&current_line) {
                if self.reading_multiline_value {
                    return Err(ReplyParseFailed(
                        "found multi-line start reply but still reading a multi-line reply"
                            .to_string(),
                    ));
                }
                self.pending_reply.push(current_line);
                self.reading_multiline_value = true;
            // multiline data to be squashed to a single entry
            } else {
                if !self.reading_multiline_value {
                    return Err(ReplyParseFailed(
                        "found a multi-line intermediate reply but not reading a multi-line reply"
                            .to_string(),
                    ));
                }
                // don't bother writing the end of multiline token
                if current_line == "." {
                    self.reading_multiline_value = false;
                } else {
                    let multiline = match self.pending_reply.last_mut() {
                        Some(multiline) => multiline,
                        // if our logic here is right, then
                        // self.reading_multiline_value == !self.pending_reply.is_empty()
                        // should always be true regardless of the data received
                        // from the control port
                        None => unreachable!(),
                    };
                    multiline.push('\n');
                    multiline.push_str(&current_line);
                }
            }
        }

        // take ownership of the reply lines
        let mut reply_lines: Vec<String> = Default::default();
        std::mem::swap(&mut self.pending_reply, &mut reply_lines);

        // parse out the response code for easier matching
        let status_code_string = match reply_lines.first() {
            Some(line) => line[0..3].to_string(),
            // the lines have already been parsed+validated in the above loop
            None => unreachable!(),
        };
        let status_code: u32 = match status_code_string.parse() {
            Ok(status_code) => status_code,
            Err(_) => {
                return Err(ReplyParseFailed(format!(
                    "unable to parse '{}' as status code",
                    status_code_string
                )))
            }
        };

        // strip the redundant status code from start of lines
        for line in reply_lines.iter_mut() {
            println!(">>> {}", line);
            if line.starts_with(&status_code_string) {
                *line = line[4..].to_string();
            }
        }

        Ok(Some(Reply {
            status_code,
            reply_lines,
        }))
    }

    pub fn write(&mut self, cmd: &str) -> Result<(), ControlStreamError> {
        println!("<<< {}", cmd);
        if let Err(err) = write!(self.stream, "{}\r\n", cmd) {
            self.closed_by_remote = true;
            return Err(ControlStreamError::WriteFailed(err));
        }
        Ok(())
    }
}

// Per-command data
#[derive(Default)]
pub struct AddOnionFlags {
    pub discard_pk: bool,
    pub detach: bool,
    pub v3_auth: bool,
    pub non_anonymous: bool,
    pub max_streams_close_circuit: bool,
}

#[derive(Default)]
pub struct OnionClientAuthAddFlags {
    pub permanent: bool,
}

// see version-spec.txt
#[derive(Clone)]
pub struct TorVersion {
    pub major: u32,
    pub minor: u32,
    pub micro: u32,
    pub patch_level: u32,
    pub status_tag: Option<String>,
}

impl TorVersion {
    fn status_tag_pattern_is_match(status_tag: &str) -> bool {
        if status_tag.is_empty() {
            return false;
        }

        for c in status_tag.chars() {
            if c.is_whitespace() {
                return false;
            }
        }
        true
    }

    fn new(
        major: u32,
        minor: u32,
        micro: u32,
        patch_level: Option<u32>,
        status_tag: Option<&str>,
    ) -> Result<TorVersion, TorVersionError> {
        let status_tag = if let Some(status_tag) = status_tag {
            if Self::status_tag_pattern_is_match(status_tag) {
                Some(status_tag.to_string())
            } else {
                return Err(TorVersionError::ParseError(
                    "tor version status tag may not be empty or contain white-space".to_string(),
                ));
            }
        } else {
            None
        };

        Ok(TorVersion {
            major,
            minor,
            micro,
            patch_level: patch_level.unwrap_or(0u32),
            status_tag,
        })
    }
}

impl FromStr for TorVersion {
    type Err = TorVersionError;

    fn from_str(s: &str) -> Result<TorVersion, Self::Err> {
        // MAJOR.MINOR.MICRO[.PATCHLEVEL][-STATUS_TAG][ (EXTRA_INFO)]*
        let mut tokens = s.split(' ');
        let (major, minor, micro, patch_level, status_tag) =
            if let Some(version_status_tag) = tokens.next() {
                let mut tokens = version_status_tag.split('-');
                let (major, minor, micro, patch_level) = if let Some(version) = tokens.next() {
                    let mut tokens = version.split('.');
                    let major: u32 = if let Some(major) = tokens.next() {
                        match major.parse() {
                            Ok(major) => major,
                            Err(_) => {
                                return Err(ParseError(format!(
                                    "failed to parse '{}' as MAJOR portion of tor version",
                                    major
                                )))
                            }
                        }
                    } else {
                        return Err(ParseError(
                            "failed to find MAJOR portion of tor version".to_string(),
                        ));
                    };
                    let minor: u32 = if let Some(minor) = tokens.next() {
                        match minor.parse() {
                            Ok(minor) => minor,
                            Err(_) => {
                                return Err(ParseError(format!(
                                    "failed to parse '{}' as MINOR portion of tor version",
                                    minor
                                )))
                            }
                        }
                    } else {
                        return Err(ParseError(
                            "failed to find MINOR portion of tor version".to_string(),
                        ));
                    };
                    let micro: u32 = if let Some(micro) = tokens.next() {
                        match micro.parse() {
                            Ok(micro) => micro,
                            Err(_) => {
                                return Err(ParseError(format!(
                                    "failed to parse '{}' as MICRO portion of tor version",
                                    micro
                                )))
                            }
                        }
                    } else {
                        return Err(ParseError(
                            "failed to find MICRO portion of tor version".to_string(),
                        ));
                    };
                    let patch_level: u32 = if let Some(patch_level) = tokens.next() {
                        match patch_level.parse() {
                            Ok(patch_level) => patch_level,
                            Err(_) => {
                                return Err(ParseError(format!(
                                    "failed to parse '{}' as PATCHLEVEL portion of tor version",
                                    patch_level
                                )))
                            }
                        }
                    } else {
                        0u32
                    };
                    (major, minor, micro, patch_level)
                } else {
                    // if there were '-' the previous next() would have returned the enire string
                    unreachable!();
                };
                let status_tag = tokens.next().map(|status_tag| status_tag.to_string());

                (major, minor, micro, patch_level, status_tag)
            } else {
                // if there were no ' ' character the previou snext() would have returned the enire string
                unreachable!();
            };
        for extra_info in tokens {
            if !extra_info.starts_with('(') || !extra_info.ends_with(')') {
                return Err(ParseError(format!(
                    "failed to parse '{}' as [ (EXTRA_INFO)]",
                    extra_info
                )));
            }
        }
        TorVersion::new(
            major,
            minor,
            micro,
            Some(patch_level),
            status_tag.as_deref(),
        )
    }
}

impl ToString for TorVersion {
    fn to_string(&self) -> String {
        match &self.status_tag {
            Some(status_tag) => format!(
                "{}.{}.{}.{}-{}",
                self.major, self.minor, self.micro, self.patch_level, status_tag
            ),
            None => format!(
                "{}.{}.{}.{}",
                self.major, self.minor, self.micro, self.patch_level
            ),
        }
    }
}

impl PartialEq for TorVersion {
    fn eq(&self, other: &Self) -> bool {
        self.major == other.major
            && self.minor == other.minor
            && self.micro == other.micro
            && self.patch_level == other.patch_level
            && self.status_tag == other.status_tag
    }
}

impl PartialOrd for TorVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if let Some(order) = self.major.partial_cmp(&other.major) {
            if order != Ordering::Equal {
                return Some(order);
            }
        }

        if let Some(order) = self.minor.partial_cmp(&other.minor) {
            if order != Ordering::Equal {
                return Some(order);
            }
        }

        if let Some(order) = self.micro.partial_cmp(&other.micro) {
            if order != Ordering::Equal {
                return Some(order);
            }
        }

        if let Some(order) = self.patch_level.partial_cmp(&other.patch_level) {
            if order != Ordering::Equal {
                return Some(order);
            }
        }

        // version-spect.txt *does* say that we should compare tags lexicgraphically
        // if all of the version numbers are the same when comparing, but we are
        // going to diverge here and say we can only compare tags for equality.
        //
        // In practice we will be comparing tor daemon tags against tagless (stable)
        // versions so this shouldn't be an issue

        if self.status_tag == other.status_tag {
            return Some(Ordering::Equal);
        }

        None
    }
}

enum AsyncEvent {
    Unknown {
        lines: Vec<String>,
    },
    StatusClient {
        severity: String,
        action: String,
        arguments: Vec<(String, String)>,
    },
    HsDesc {
        action: String,
        hs_address: V3OnionServiceId,
    },
}

struct TorController {
    // underlying control stream
    control_stream: ControlStream,
    // list of async replies to be handled
    async_replies: Vec<Reply>,
    // regex for parsing events
    status_event_pattern: Regex,
    status_event_argument_pattern: Regex,
    hs_desc_pattern: Regex,
}

impl TorController {
    pub fn new(control_stream: ControlStream) -> Result<TorController, TorControllerError> {
        let status_event_pattern =
            Regex::new(r#"^STATUS_CLIENT (?P<severity>NOTICE|WARN|ERR) (?P<action>[A-Za-z]+)"#)
                .map_err(TorControllerError::ParsingRegexCreationFailed)?;
        let status_event_argument_pattern =
            Regex::new(r#"(?P<key>[A-Z]+)=(?P<value>[A-Za-z0-9_]+|"[^"]+")"#)
                .map_err(TorControllerError::ParsingRegexCreationFailed)?;
        let hs_desc_pattern = Regex::new(
            r#"HS_DESC (?P<action>REQUESTED|UPLOAD|RECEIVED|UPLOADED|IGNORE|FAILED|CREATED) (?P<hsaddress>[a-z2-7]{56})"#
        ).map_err(TorControllerError::ParsingRegexCreationFailed)?;

        Ok(TorController {
            control_stream,
            async_replies: Default::default(),
            // regex
            status_event_pattern,
            status_event_argument_pattern,
            hs_desc_pattern,
        })
    }

    // return curently available events, does not block waiting
    // for an event
    fn wait_async_replies(&mut self) -> Result<Vec<Reply>, TorControllerError> {
        let mut replies: Vec<Reply> = Default::default();
        // take any previously received async replies
        std::mem::swap(&mut self.async_replies, &mut replies);

        // and keep consuming until none are available
        loop {
            if let Some(reply) = self.control_stream.read_reply().map_err(ReadReplyFailed)? {
                replies.push(reply);
            } else {
                // no more replies immediately available so return
                return Ok(replies);
            }
        }
    }

    fn reply_to_event(&self, reply: &mut Reply) -> Result<AsyncEvent, TorControllerError> {
        if reply.status_code != 650u32 {
            return Err(UnexpectedSynchonousReplyReceived());
        }

        // not sure this is what we want but yolo
        let reply_text = reply.reply_lines.join(" ");
        if let Some(caps) = self.status_event_pattern.captures(&reply_text) {
            let severity = match caps.name("severity") {
                Some(severity) => severity.as_str(),
                None => unreachable!(),
            };
            let action = match caps.name("action") {
                Some(action) => action.as_str(),
                None => unreachable!(),
            };

            let mut arguments: Vec<(String, String)> = Default::default();
            for caps in self
                .status_event_argument_pattern
                .captures_iter(&reply_text)
            {
                let key = match caps.name("key") {
                    Some(key) => key.as_str(),
                    None => unreachable!(),
                };
                let value = {
                    let value = match caps.name("value") {
                        Some(value) => value.as_str(),
                        None => unreachable!(),
                    };
                    if value.starts_with('\"') && value.ends_with('\"') {
                        &value[1..value.len() - 1]
                    } else {
                        value
                    }
                };
                arguments.push((key.to_string(), value.to_string()));
            }

            return Ok(AsyncEvent::StatusClient {
                severity: severity.to_string(),
                action: action.to_string(),
                arguments,
            });
        }

        if let Some(caps) = self.hs_desc_pattern.captures(&reply_text) {
            let action = match caps.name("action") {
                Some(action) => action.as_str(),
                None => unreachable!(),
            };
            let hs_address = match caps.name("hsaddress") {
                Some(hs_address) => hs_address.as_str(),
                None => unreachable!(),
            };

            if let Ok(hs_address) = V3OnionServiceId::from_string(hs_address) {
                return Ok(AsyncEvent::HsDesc {
                    action: action.to_string(),
                    hs_address,
                });
            }
        }

        // no luck parsing reply, just return full text
        let mut reply_lines: Vec<String> = Default::default();
        std::mem::swap(&mut reply_lines, &mut reply.reply_lines);

        Ok(AsyncEvent::Unknown { lines: reply_lines })
    }

    pub fn wait_async_events(&mut self) -> Result<Vec<AsyncEvent>, TorControllerError> {
        let mut async_replies = self.wait_async_replies()?;
        let mut async_events: Vec<AsyncEvent> = Default::default();

        for reply in async_replies.iter_mut() {
            async_events.push(self.reply_to_event(reply)?);
        }

        Ok(async_events)
    }

    // wait for a sync reply, save off async replies for later
    fn wait_sync_reply(&mut self) -> Result<Reply, TorControllerError> {
        loop {
            if let Some(reply) = self.control_stream.read_reply().map_err(ReadReplyFailed)? {
                match reply.status_code {
                    650u32 => self.async_replies.push(reply),
                    _ => return Ok(reply),
                }
            }
        }
    }

    fn write_command(&mut self, text: &str) -> Result<Reply, TorControllerError> {
        self.control_stream
            .write(text)
            .map_err(WriteCommandFailed)?;
        self.wait_sync_reply()
    }

    //
    // Tor Commands
    //
    // The section where we can find the specification in control-spec.txt
    // for the underlying command is listed in parentheses
    //
    // Each of these command wrapper methods block until completion
    //

    // SETCONF (3.1)
    fn setconf_cmd(&mut self, key_values: &[(&str, &str)]) -> Result<Reply, TorControllerError> {
        if key_values.is_empty() {
            return Err(TorControllerError::InvalidCommandArguments(
                "SETCONF key-value pairs list must not be empty".to_string(),
            ));
        }
        let mut command_buffer = vec!["SETCONF".to_string()];

        for (key, value) in key_values.iter() {
            command_buffer.push(format!("{}={}", key, value));
        }
        let command = command_buffer.join(" ");

        self.write_command(&command)
    }

    // GETCONF (3.3)
    #[cfg(test)]
    fn getconf_cmd(&mut self, keywords: &[&str]) -> Result<Reply, TorControllerError> {
        if keywords.is_empty() {
            return Err(TorControllerError::InvalidCommandArguments(
                "GETCONF keywords list must not be empty".to_string(),
            ));
        }
        let command = format!("GETCONF {}", keywords.join(" "));

        self.write_command(&command)
    }

    // SETEVENTS (3.4)
    fn setevents_cmd(&mut self, event_codes: &[&str]) -> Result<Reply, TorControllerError> {
        if event_codes.is_empty() {
            return Err(TorControllerError::InvalidCommandArguments(
                "SETEVENTS event codes list mut not be empty".to_string(),
            ));
        }
        let command = format!("SETEVENTS {}", event_codes.join(" "));

        self.write_command(&command)
    }

    // AUTHENTICATE (3.5)
    fn authenticate_cmd(&mut self, password: &str) -> Result<Reply, TorControllerError> {
        let command = format!("AUTHENTICATE \"{}\"", password);

        self.write_command(&command)
    }

    // GETINFO (3.9)
    fn getinfo_cmd(&mut self, keywords: &[&str]) -> Result<Reply, TorControllerError> {
        if keywords.is_empty() {
            return Err(TorControllerError::InvalidCommandArguments(
                "GETINFO keywords list must not be empty".to_string(),
            ));
        }
        let command = format!("GETINFO {}", keywords.join(" "));

        self.write_command(&command)
    }

    // ADD_ONION (3.27)
    fn add_onion_cmd(
        &mut self,
        key: Option<&Ed25519PrivateKey>,
        flags: &AddOnionFlags,
        max_streams: Option<u16>,
        virt_port: u16,
        target: Option<SocketAddr>,
        client_auth: Option<&[X25519PublicKey]>,
    ) -> Result<Reply, TorControllerError> {
        let mut command_buffer = vec!["ADD_ONION".to_string()];

        // set our key or request a new one
        if let Some(key) = key {
            command_buffer.push(key.to_key_blob());
        } else {
            command_buffer.push("NEW:ED25519-V3".to_string());
        }

        // set our flags
        let mut flag_buffer: Vec<&str> = Default::default();
        if flags.discard_pk {
            flag_buffer.push("DiscardPK");
        }
        if flags.detach {
            flag_buffer.push("Detach");
        }
        if flags.v3_auth {
            flag_buffer.push("V3Auth");
        }
        if flags.non_anonymous {
            flag_buffer.push("NonAnonymous");
        }
        if flags.max_streams_close_circuit {
            flag_buffer.push("MaxStreamsCloseCircuit");
        }

        if !flag_buffer.is_empty() {
            command_buffer.push(format!("Flags={}", flag_buffer.join(",")));
        }

        // set max concurrent streams
        if let Some(max_streams) = max_streams {
            command_buffer.push(format!("MaxStreams={}", max_streams));
        }

        // set our onion service target
        if let Some(target) = target {
            command_buffer.push(format!("Port={},{}", virt_port, target));
        } else {
            command_buffer.push(format!("Port={}", virt_port));
        }
        // setup client auth
        if let Some(client_auth) = client_auth {
            for key in client_auth.iter() {
                command_buffer.push(format!("ClientAuthV3={}", key.to_base32()));
            }
        }

        // finally send the command
        let command = command_buffer.join(" ");

        self.write_command(&command)
    }

    // DEL_ONION (3.38)
    fn del_onion_cmd(
        &mut self,
        service_id: &V3OnionServiceId,
    ) -> Result<Reply, TorControllerError> {
        let command = format!("DEL_ONION {}", service_id.to_string());

        self.write_command(&command)
    }

    // ONION_CLIENT_AUTH_ADD (3.30)
    fn onion_client_auth_add_cmd(
        &mut self,
        service_id: &V3OnionServiceId,
        private_key: &X25519PrivateKey,
        client_name: Option<String>,
        flags: &OnionClientAuthAddFlags,
    ) -> Result<Reply, TorControllerError> {
        let mut command_buffer = vec!["ONION_CLIENT_AUTH_ADD".to_string()];

        // set the onion service id
        command_buffer.push(service_id.to_string());

        // set our client's private key
        command_buffer.push(format!("x25519:{}", private_key.to_base64()));

        if let Some(client_name) = client_name {
            command_buffer.push(format!("ClientName={}", client_name));
        }

        if flags.permanent {
            command_buffer.push("Flags=Permanent".to_string());
        }

        // finally send command
        let command = command_buffer.join(" ");

        self.write_command(&command)
    }

    // ONION_CLIENT_AUTH_REMOVE (3.31)
    fn onion_client_auth_remove_cmd(
        &mut self,
        service_id: &V3OnionServiceId,
    ) -> Result<Reply, TorControllerError> {
        let command = format!("ONION_CLIENT_AUTH_REMOVE {}", service_id.to_string());

        self.write_command(&command)
    }

    //
    // Public high-level typesafe command method wrappers
    //

    pub fn setconf(&mut self, key_values: &[(&str, &str)]) -> Result<(), TorControllerError> {
        let reply = self.setconf_cmd(key_values)?;

        match reply.status_code {
            250u32 => Ok(()),
            code => Err(TorControllerError::CommandReturnedError(
                code,
                reply.reply_lines,
            )),
        }
    }

    #[cfg(test)]
    pub fn getconf(
        &mut self,
        keywords: &[&str],
    ) -> Result<Vec<(String, String)>, TorControllerError> {
        let reply = self.getconf_cmd(keywords)?;

        match reply.status_code {
            250u32 => {
                let mut key_values: Vec<(String, String)> = Default::default();
                for line in reply.reply_lines {
                    match line.find('=') {
                        Some(index) => key_values
                            .push((line[0..index].to_string(), line[index + 1..].to_string())),
                        None => key_values.push((line, String::new())),
                    }
                }
                Ok(key_values)
            }
            code => Err(TorControllerError::CommandReturnedError(
                code,
                reply.reply_lines,
            )),
        }
    }

    pub fn setevents(&mut self, events: &[&str]) -> Result<(), TorControllerError> {
        let reply = self.setevents_cmd(events)?;

        match reply.status_code {
            250u32 => Ok(()),
            code => Err(TorControllerError::CommandReturnedError(
                code,
                reply.reply_lines,
            )),
        }
    }

    pub fn authenticate(&mut self, password: &str) -> Result<(), TorControllerError> {
        let reply = self.authenticate_cmd(password)?;

        match reply.status_code {
            250u32 => Ok(()),
            code => Err(TorControllerError::CommandReturnedError(
                code,
                reply.reply_lines,
            )),
        }
    }

    pub fn getinfo(
        &mut self,
        keywords: &[&str],
    ) -> Result<Vec<(String, String)>, TorControllerError> {
        let reply = self.getinfo_cmd(keywords)?;

        match reply.status_code {
            250u32 => {
                let mut key_values: Vec<(String, String)> = Default::default();
                for line in reply.reply_lines {
                    match line.find('=') {
                        Some(index) => key_values
                            .push((line[0..index].to_string(), line[index + 1..].to_string())),
                        None => {
                            if line != "OK" {
                                key_values.push((line, String::new()))
                            }
                        }
                    }
                }
                Ok(key_values)
            }
            code => Err(TorControllerError::CommandReturnedError(
                code,
                reply.reply_lines,
            )),
        }
    }

    pub fn add_onion(
        &mut self,
        key: Option<&Ed25519PrivateKey>,
        flags: &AddOnionFlags,
        max_streams: Option<u16>,
        virt_port: u16,
        target: Option<SocketAddr>,
        client_auth: Option<&[X25519PublicKey]>,
    ) -> Result<(Option<Ed25519PrivateKey>, V3OnionServiceId), TorControllerError> {
        let reply = self.add_onion_cmd(key, flags, max_streams, virt_port, target, client_auth)?;

        let mut private_key: Option<Ed25519PrivateKey> = None;
        let mut service_id: Option<V3OnionServiceId> = None;

        match reply.status_code {
            250u32 => {
                for line in reply.reply_lines {
                    if let Some(mut index) = line.find("ServiceID=") {
                        if service_id.is_some() {
                            return Err(TorControllerError::CommandReplyParseFailed(
                                "received duplicate ServiceID entries".to_string(),
                            ));
                        }
                        index += "ServiceId=".len();
                        let service_id_string = &line[index..];
                        service_id = match V3OnionServiceId::from_string(service_id_string) {
                            Ok(service_id) => Some(service_id),
                            Err(_) => {
                                return Err(TorControllerError::CommandReplyParseFailed(format!(
                                    "could not parse '{}' as V3OnionServiceId",
                                    service_id_string
                                )))
                            }
                        }
                    } else if let Some(mut index) = line.find("PrivateKey=") {
                        if private_key.is_some() {
                            return Err(TorControllerError::CommandReplyParseFailed(
                                "received duplicate PrivateKey entries".to_string(),
                            ));
                        }
                        index += "PrivateKey=".len();
                        let key_blob_string = &line[index..];
                        private_key = match Ed25519PrivateKey::from_key_blob(key_blob_string) {
                            Ok(private_key) => Some(private_key),
                            Err(_) => {
                                return Err(TorControllerError::CommandReplyParseFailed(format!(
                                    "could not parse {} as Ed25519PrivateKey",
                                    key_blob_string
                                )))
                            }
                        };
                    } else if line.contains("ClientAuthV3=") {
                        if client_auth.unwrap_or_default().is_empty() {
                            return Err(TorControllerError::CommandReplyParseFailed(
                                "recieved unexpected ClientAuthV3 keys".to_string(),
                            ));
                        }
                    } else if !line.contains("OK") {
                        return Err(TorControllerError::CommandReplyParseFailed(format!(
                            "received unexpected reply line '{}'",
                            line
                        )));
                    }
                }
            }
            code => {
                return Err(TorControllerError::CommandReturnedError(
                    code,
                    reply.reply_lines,
                ))
            }
        }

        if flags.discard_pk {
            if private_key.is_some() {
                return Err(TorControllerError::CommandReplyParseFailed(
                    "PrivateKey response should have been discard".to_string(),
                ));
            }
        } else if private_key.is_none() {
            return Err(TorControllerError::CommandReplyParseFailed(
                "did not receive a PrivateKey".to_string(),
            ));
        }

        match service_id {
            Some(service_id) => Ok((private_key, service_id)),
            None => Err(TorControllerError::CommandReplyParseFailed(
                "did not receive a ServiceID".to_string(),
            )),
        }
    }

    pub fn del_onion(&mut self, service_id: &V3OnionServiceId) -> Result<(), TorControllerError> {
        let reply = self.del_onion_cmd(service_id)?;

        match reply.status_code {
            250u32 => Ok(()),
            code => Err(TorControllerError::CommandReturnedError(
                code,
                reply.reply_lines,
            )),
        }
    }

    // more specific encapulsation of specific command invocations

    pub fn getinfo_net_listeners_socks(&mut self) -> Result<Vec<SocketAddr>, TorControllerError> {
        let response = self.getinfo(&["net/listeners/socks"])?;
        for (key, value) in response.iter() {
            if key.as_str() == "net/listeners/socks" {
                if value.is_empty() {
                    return Ok(Default::default());
                }
                // get our list of double-quoted strings
                let listeners: Vec<&str> = value.split(' ').collect();
                let mut result: Vec<SocketAddr> = Default::default();
                for socket_addr in listeners.iter() {
                    if !socket_addr.starts_with('\"') || !socket_addr.ends_with('\"') {
                        return Err(TorControllerError::CommandReplyParseFailed(format!(
                            "could not parse '{}' as socket address",
                            socket_addr
                        )));
                    }

                    // remove leading/trailing double quote
                    let stripped = &socket_addr[1..socket_addr.len() - 1];
                    result.push(match SocketAddr::from_str(stripped) {
                        Ok(result) => result,
                        Err(_) => {
                            return Err(TorControllerError::CommandReplyParseFailed(format!(
                                "could not parse '{}' as socket address",
                                socket_addr
                            )))
                        }
                    });
                }
                return Ok(result);
            }
        }
        Err(TorControllerError::CommandReplyParseFailed(
            "reply did not find a 'net/listeners/socks' key/value".to_string(),
        ))
    }

    pub fn getinfo_version(&mut self) -> Result<TorVersion, TorControllerError> {
        let response = self.getinfo(&["version"])?;
        for (key, value) in response.iter() {
            if key.as_str() == "version" {
                return TorVersion::from_str(value).map_err(TorVersionParseFailed);
            }
        }
        Err(TorControllerError::CommandReplyParseFailed(
            "did not find a 'version' key/value".to_string(),
        ))
    }

    pub fn onion_client_auth_add(
        &mut self,
        service_id: &V3OnionServiceId,
        private_key: &X25519PrivateKey,
        client_name: Option<String>,
        flags: &OnionClientAuthAddFlags,
    ) -> Result<(), TorControllerError> {
        let reply = self.onion_client_auth_add_cmd(service_id, private_key, client_name, flags)?;

        match reply.status_code {
            250u32..=252u32 => Ok(()),
            code => Err(TorControllerError::CommandReturnedError(
                code,
                reply.reply_lines,
            )),
        }
    }

    #[allow(dead_code)]
    pub fn onion_client_auth_remove(
        &mut self,
        service_id: &V3OnionServiceId,
    ) -> Result<(), TorControllerError> {
        let reply = self.onion_client_auth_remove_cmd(service_id)?;

        match reply.status_code {
            250u32..=251u32 => Ok(()),
            code => Err(TorControllerError::CommandReturnedError(
                code,
                reply.reply_lines,
            )),
        }
    }
}

pub struct CircuitToken {
    username: String,
    password: String,
}

impl CircuitToken {
    #[allow(dead_code)]
    pub fn new(first_party: Host) -> CircuitToken {
        const CIRCUIT_TOKEN_PASSWORD_LENGTH: usize = 32usize;
        let username = first_party.to_string();
        let password = generate_password(CIRCUIT_TOKEN_PASSWORD_LENGTH);

        CircuitToken { username, password }
    }
}

pub struct OnionStream {
    stream: TcpStream,
    peer_addr: Option<V3OnionServiceId>,
}

impl OnionStream {
    pub fn nodelay(&self) -> Result<bool, std::io::Error> {
        self.stream.nodelay()
    }

    pub fn peer_addr(&self) -> Option<&V3OnionServiceId> {
        self.peer_addr.as_ref()
    }

    pub fn read_timeout(&self) -> Result<Option<Duration>, std::io::Error> {
        self.stream.read_timeout()
    }

    pub fn set_nodelay(&self, nodelay: bool) -> Result<(), std::io::Error> {
        self.stream.set_nodelay(nodelay)
    }

    pub fn set_nonblocking(&self, nonblocking: bool) -> Result<(), std::io::Error> {
        self.stream.set_nonblocking(nonblocking)
    }

    pub fn set_read_timeout(&self, dur: Option<Duration>) -> Result<(), std::io::Error> {
        self.stream.set_read_timeout(dur)
    }

    pub fn set_write_timeout(&self, dur: Option<Duration>) -> Result<(), std::io::Error> {
        self.stream.set_write_timeout(dur)
    }

    pub fn shutdown(&self, how: std::net::Shutdown) -> Result<(), std::io::Error> {
        self.stream.shutdown(how)
    }

    pub fn take_error(&self) -> Result<Option<std::io::Error>, std::io::Error> {
        self.stream.take_error()
    }

    pub fn write_timeout(&self) -> Result<Option<Duration>, std::io::Error> {
        self.stream.write_timeout()
    }

    pub fn try_clone(&self) -> Result<OnionStream, std::io::Error> {
        Ok(OnionStream {
            stream: self.stream.try_clone()?,
            peer_addr: self.peer_addr.clone(),
        })
    }
}

// pass-through to underlying Read stream
impl Read for OnionStream {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, std::io::Error> {
        self.stream.read(buf)
    }
}

// pass-through to underlying Write stream
impl Write for OnionStream {
    fn write(&mut self, buf: &[u8]) -> Result<usize, std::io::Error> {
        self.stream.write(buf)
    }

    fn flush(&mut self) -> Result<(), std::io::Error> {
        self.stream.flush()
    }
}

impl From<OnionStream> for TcpStream {
    fn from(onion_stream: OnionStream) -> Self {
        onion_stream.stream
    }
}

pub struct OnionListener {
    listener: TcpListener,
    is_active: Arc<atomic::AtomicBool>,
}

impl OnionListener {
    pub fn set_nonblocking(&self, nonblocking: bool) -> Result<(), std::io::Error> {
        self.listener.set_nonblocking(nonblocking)
    }

    pub fn accept(&self) -> Result<Option<OnionStream>, std::io::Error> {
        match self.listener.accept() {
            Ok((stream, _socket_addr)) => Ok(Some(OnionStream {
                stream,
                peer_addr: None,
            })),
            Err(err) => {
                if err.kind() == ErrorKind::WouldBlock {
                    Ok(None)
                } else {
                    Err(err)
                }
            }
        }
    }
}

impl Drop for OnionListener {
    fn drop(&mut self) {
        self.is_active.store(false, atomic::Ordering::Relaxed);
    }
}

pub enum Event {
    BootstrapStatus {
        progress: u32,
        tag: String,
        summary: String,
    },
    BootstrapComplete,
    LogReceived {
        line: String,
    },
    OnionServicePublished {
        service_id: V3OnionServiceId,
    },
}

pub struct TorManager {
    daemon: TorProcess,
    version: TorVersion,
    controller: TorController,
    socks_listener: Option<SocketAddr>,
    // list of open onion services and their is_active flag
    onion_services: Vec<(V3OnionServiceId, Arc<atomic::AtomicBool>)>,
}

impl TorManager {
    pub fn new(tor_bin_path: &Path, data_directory: &Path) -> Result<TorManager, TorManagerError> {
        // launch tor
        let daemon =
            TorProcess::new(tor_bin_path, data_directory).map_err(TorProcessCreationFailed)?;
        // open a control stream
        let control_stream = ControlStream::new(&daemon.control_addr, Duration::from_millis(16))
            .map_err(ControlStreamCreationFailed)?;

        // create a controler
        let mut controller =
            TorController::new(control_stream).map_err(TorControllerCreationFailed)?;

        // authenticate
        controller
            .authenticate(&daemon.password)
            .map_err(TorProcessAuthenticationFailed)?;

        let min_required_version: TorVersion = TorVersion {
            major: 0u32,
            minor: 4u32,
            micro: 6u32,
            patch_level: 1u32,
            status_tag: None,
        };

        let version = controller.getinfo_version().map_err(GetInfoVersionFailed)?;

        if version < min_required_version {
            return Err(TorManagerError::TorProcessTooOld(
                version.to_string(),
                min_required_version.to_string(),
            ));
        }

        // register for STATUS_CLIENT async events
        controller
            .setevents(&["STATUS_CLIENT", "HS_DESC"])
            .map_err(SetEventsFailed)?;

        Ok(TorManager {
            daemon,
            version,
            controller,
            socks_listener: None,
            onion_services: Default::default(),
        })
    }

    pub fn update(&mut self) -> Result<Vec<Event>, TorManagerError> {
        let mut i = 0;
        while i < self.onion_services.len() {
            // remove onion services with no active listeners
            if !self.onion_services[i].1.load(atomic::Ordering::Relaxed) {
                let entry = self.onion_services.swap_remove(i);
                let service_id = entry.0;

                self.controller
                    .del_onion(&service_id)
                    .map_err(DelOnionFailed)?;
            } else {
                i += 1;
            }
        }

        let mut events: Vec<Event> = Default::default();
        for async_event in self
            .controller
            .wait_async_events()
            .map_err(WaitAsyncEventsFailed)?
            .iter()
        {
            match async_event {
                AsyncEvent::StatusClient {
                    severity,
                    action,
                    arguments,
                } => {
                    if severity == "NOTICE" && action == "BOOTSTRAP" {
                        let mut progress: u32 = 0;
                        let mut tag: String = Default::default();
                        let mut summary: String = Default::default();
                        for (key, val) in arguments.iter() {
                            match key.as_str() {
                                "PROGRESS" => progress = val.parse().unwrap_or(0u32),
                                "TAG" => tag = val.to_string(),
                                "SUMMARY" => summary = val.to_string(),
                                _ => {} // ignore unexpected arguments
                            }
                        }
                        events.push(Event::BootstrapStatus {
                            progress,
                            tag,
                            summary,
                        });
                        if progress == 100u32 {
                            events.push(Event::BootstrapComplete);
                        }
                    }
                }
                AsyncEvent::HsDesc { action, hs_address } => {
                    if action == "UPLOADED" {
                        events.push(Event::OnionServicePublished {
                            service_id: hs_address.clone(),
                        });
                    }
                }
                AsyncEvent::Unknown { lines } => {
                    println!("Received Unknown Event:");
                    for line in lines.iter() {
                        println!(" {}", line);
                    }
                }
            }
        }

        for log_line in self.daemon.wait_log_lines().iter_mut() {
            events.push(Event::LogReceived {
                line: std::mem::take(log_line),
            });
        }

        Ok(events)
    }

    #[allow(dead_code)]
    pub fn version(&mut self) -> TorVersion {
        self.version.clone()
    }

    pub fn bootstrap(&mut self) -> Result<(), TorManagerError> {
        self.controller
            .setconf(&[("DisableNetwork", "0")])
            .map_err(SetConfDisableNetwork0Failed)
    }

    pub fn add_client_auth(
        &mut self,
        service_id: &V3OnionServiceId,
        client_auth: &X25519PrivateKey,
    ) -> Result<(), TorManagerError> {
        self.controller
            .onion_client_auth_add(service_id, client_auth, None, &Default::default())
            .map_err(SetConfDisableNetwork0Failed)
    }

    pub fn remove_client_auth(
        &mut self,
        service_id: &V3OnionServiceId,
    ) -> Result<(), TorManagerError> {
        self.controller
            .onion_client_auth_remove(service_id)
            .map_err(OnionClientAuthRemoveFailed)
    }

    // connect to an onion service and returns OnionStream
    pub fn connect(
        &mut self,
        service_id: &V3OnionServiceId,
        virt_port: u16,
        circuit: Option<CircuitToken>,
    ) -> Result<OnionStream, TorManagerError> {
        if self.socks_listener.is_none() {
            let mut listeners = self
                .controller
                .getinfo_net_listeners_socks()
                .map_err(GetInfoNetListenersSocksFailed)?;
            if listeners.is_empty() {
                return Err(TorManagerError::NoSocksListenersFound());
            }
            self.socks_listener = Some(listeners.swap_remove(0));
        }

        let socks_listener = match self.socks_listener {
            Some(socks_listener) => socks_listener,
            None => unreachable!(),
        };

        // our onion domain
        let target =
            socks::TargetAddr::Domain(format!("{}.onion", service_id.to_string()), virt_port);
        // readwrite stream
        let stream = match &circuit {
            None => Socks5Stream::connect(socks_listener, target),
            Some(circuit) => Socks5Stream::connect_with_password(
                socks_listener,
                target,
                &circuit.username,
                &circuit.password,
            ),
        }
        .map_err(Socks5ConnectionFailed)?;

        Ok(OnionStream {
            stream: stream.into_inner(),
            peer_addr: Some(service_id.clone()),
        })
    }

    // stand up an onion service and return an OnionListener
    pub fn listener(
        &mut self,
        private_key: &Ed25519PrivateKey,
        virt_port: u16,
        authorized_clients: Option<&[X25519PublicKey]>,
    ) -> Result<OnionListener, TorManagerError> {
        // try to bind to a local address, let OS pick our port
        let socket_addr = SocketAddr::from(([127, 0, 0, 1], 0u16));
        let listener = TcpListener::bind(socket_addr).map_err(TcpListenerBindFailed)?;
        let socket_addr = listener.local_addr().map_err(TcpListenerLocalAddrFailed)?;

        let mut flags = AddOnionFlags {
            discard_pk: true,
            ..Default::default()
        };
        if authorized_clients.is_some() {
            flags.v3_auth = true;
        }

        // start onion service
        let (_, service_id) = self
            .controller
            .add_onion(
                Some(private_key),
                &flags,
                None,
                virt_port,
                Some(socket_addr),
                authorized_clients,
            )
            .map_err(AddOnionFailed)?;

        let is_active = Arc::new(atomic::AtomicBool::new(true));
        self.onion_services
            .push((service_id, Arc::clone(&is_active)));

        Ok(OnionListener {
            listener,
            is_active,
        })
    }
}

#[test]
#[serial]
fn test_tor_controller() -> anyhow::Result<()> {
    let tor_path = which::which(tor_exe_name())?;
    let mut data_path = std::env::temp_dir();
    data_path.push("test_tor_controller");
    let tor_process = TorProcess::new(&tor_path, &data_path)?;

    // create a scope to ensure tor_controller is dropped
    {
        let control_stream =
            ControlStream::new(&tor_process.control_addr, Duration::from_millis(16))?;

        // create a tor controller and send authentication command
        let mut tor_controller = TorController::new(control_stream)?;
        tor_controller.authenticate_cmd(&tor_process.password)?;
        assert!(
            tor_controller
                .authenticate_cmd("invalid password")?
                .status_code
                == 515u32
        );

        // tor controller should have shutdown the connection after failed authentication
        assert!(
            tor_controller
                .authenticate_cmd(&tor_process.password)
                .is_err(),
            "expected failure due to closed connection"
        );
        assert!(tor_controller.control_stream.closed_by_remote());
    }
    // now create a second controller
    {
        let control_stream =
            ControlStream::new(&tor_process.control_addr, Duration::from_millis(16))?;

        // create a tor controller and send authentication command
        // all async events are just printed to stdout
        let mut tor_controller = TorController::new(control_stream)?;
        tor_controller.authenticate(&tor_process.password)?;

        // ensure everything is matching our default_torrc settings
        let vals = tor_controller.getconf(&["SocksPort", "AvoidDiskWrites", "DisableNetwork"])?;
        for (key, value) in vals.iter() {
            let expected = match key.as_str() {
                "SocksPort" => "auto OnionTrafficOnly",
                "AvoidDiskWrites" => "1",
                "DisableNetwork" => "1",
                _ => panic!("unexpected returned key: {}", key),
            };
            assert!(value == expected);
        }

        let vals = tor_controller.getinfo(&["version", "config-file", "config-text"])?;
        let mut expected_torrc_path = data_path.clone();
        expected_torrc_path.push("torrc");
        let mut expected_control_port_path = data_path.clone();
        expected_control_port_path.push("control_port");
        for (key, value) in vals.iter() {
            match key.as_str() {
                "version" => assert!(Regex::new(r"\d+\.\d+\.\d+\.\d+")?.is_match(&value)),
                "config-file" => assert!(Path::new(&value) == expected_torrc_path),
                "config-text" => assert!(
                    value.to_string()
                        == format!(
                            "\nControlPort auto\nControlPortWriteToFile {}\nDataDirectory {}",
                            expected_control_port_path.display(),
                            data_path.display()
                        )
                ),
                _ => panic!("unexpected returned key: {}", key),
            }
        }

        tor_controller.setevents(&["STATUS_CLIENT"])?;
        // begin bootstrap
        tor_controller.setconf(&[("DisableNetwork", "0")])?;

        // add an onoin service
        let (private_key, service_id) =
            match tor_controller.add_onion(None, &Default::default(), None, 22, None, None)? {
                (Some(private_key), service_id) => (private_key, service_id),
                _ => panic!("add_onion did not return expected values"),
            };
        println!("private_key: {}", private_key.to_key_blob());
        println!("service_id: {}", service_id.to_string());

        assert!(
            tor_controller
                .del_onion(&V3OnionServiceId::from_string(
                    "6l62fw7tqctlu5fesdqukvpoxezkaxbzllrafa2ve6ewuhzphxczsjyd"
                )?)
                .is_err(),
            "deleting unknown onion should have failed"
        );

        // delete our new onion
        tor_controller.del_onion(&service_id)?;

        println!("listeners: ");
        for sock_addr in tor_controller.getinfo_net_listeners_socks()?.iter() {
            println!(" {}", sock_addr);
        }

        // print our event names available to tor
        for (key, value) in tor_controller.getinfo(&["events/names"])?.iter() {
            println!("{} : {}", key, value);
        }

        let stop_time = Instant::now() + std::time::Duration::from_secs(5);
        while stop_time > Instant::now() {
            for async_event in tor_controller.wait_async_events()?.iter() {
                match async_event {
                    AsyncEvent::Unknown { lines } => {
                        println!("Unknown: {}", lines.join("\n"));
                    }
                    AsyncEvent::StatusClient {
                        severity,
                        action,
                        arguments,
                    } => {
                        println!("STATUS_CLIENT severity={}, action={}", severity, action);
                        for (key, value) in arguments.iter() {
                            println!(" {}='{}'", key, value);
                        }
                    }
                    AsyncEvent::HsDesc { action, hs_address } => {
                        println!(
                            "HS_DESC action={}, hsaddress={}",
                            action,
                            hs_address.to_string()
                        );
                    }
                }
            }
        }
    }

    Ok(())
}

#[test]
fn test_version() -> anyhow::Result<()> {
    assert!(TorVersion::from_str("1.2.3")? == TorVersion::new(1, 2, 3, None, None)?);
    assert!(TorVersion::from_str("1.2.3.4")? == TorVersion::new(1, 2, 3, Some(4), None)?);
    assert!(TorVersion::from_str("1.2.3-test")? == TorVersion::new(1, 2, 3, None, Some("test"))?);
    assert!(
        TorVersion::from_str("1.2.3.4-test")? == TorVersion::new(1, 2, 3, Some(4), Some("test"))?
    );
    assert!(TorVersion::from_str("1.2.3 (extra_info)")? == TorVersion::new(1, 2, 3, None, None)?);
    assert!(
        TorVersion::from_str("1.2.3.4 (extra_info)")? == TorVersion::new(1, 2, 3, Some(4), None)?
    );
    assert!(
        TorVersion::from_str("1.2.3.4-tag (extra_info)")?
            == TorVersion::new(1, 2, 3, Some(4), Some("tag"))?
    );

    assert!(
        TorVersion::from_str("1.2.3.4-tag (extra_info) (extra_info)")?
            == TorVersion::new(1, 2, 3, Some(4), Some("tag"))?
    );

    assert!(TorVersion::new(1, 2, 3, Some(4), Some("spaced tag")).is_err());
    assert!(TorVersion::new(1, 2, 3, Some(4), Some("" /* empty tag */)).is_err());
    assert!(TorVersion::from_str("").is_err());
    assert!(TorVersion::from_str("1.2").is_err());
    assert!(TorVersion::from_str("1.2-foo").is_err());
    assert!(TorVersion::from_str("1.2.3.4-foo bar").is_err());
    assert!(TorVersion::from_str("1.2.3.4-foo bar (extra_info)").is_err());
    assert!(TorVersion::from_str("1.2.3.4-foo (extra_info) badtext").is_err());
    assert!(TorVersion::new(0, 0, 0, Some(0), None)? < TorVersion::new(1, 0, 0, Some(0), None)?);
    assert!(TorVersion::new(0, 0, 0, Some(0), None)? < TorVersion::new(0, 1, 0, Some(0), None)?);
    assert!(TorVersion::new(0, 0, 0, Some(0), None)? < TorVersion::new(0, 0, 1, Some(0), None)?);

    // ensure status tags make comparison between equal versions (apart from
    // tags) unknowable
    let zero_version = TorVersion::new(0, 0, 0, Some(0), None)?;
    let zero_version_tag = TorVersion::new(0, 0, 0, Some(0), Some("tag"))?;

    assert!(!(zero_version < zero_version_tag));
    assert!(!(zero_version <= zero_version_tag));
    assert!(!(zero_version > zero_version_tag));
    assert!(!(zero_version >= zero_version_tag));

    Ok(())
}

#[test]
#[serial]
fn test_tor_manager() -> anyhow::Result<()> {
    let tor_path = which::which(tor_exe_name())?;
    let mut data_path = std::env::temp_dir();
    data_path.push("test_tor_manager");

    let mut tor = TorManager::new(&tor_path, &data_path)?;
    println!("version : {}", tor.version().to_string());
    tor.bootstrap()?;

    let mut received_log = false;
    let mut bootstrap_complete = false;
    while !bootstrap_complete {
        for event in tor.update()?.iter() {
            match event {
                Event::BootstrapStatus {
                    progress,
                    tag,
                    summary,
                } => println!(
                    "BootstrapStatus: {{ progress: {}, tag: {}, summary: '{}' }}",
                    progress, tag, summary
                ),
                Event::BootstrapComplete => {
                    println!("Bootstrap Complete!");
                    bootstrap_complete = true;
                }
                Event::LogReceived { line } => {
                    received_log = true;
                    println!("--- {}", line);
                }
                _ => {}
            }
        }
    }
    assert!(
        received_log,
        "should have received a log line from tor daemon"
    );

    Ok(())
}

#[test]
#[serial]
fn test_onion_service() -> anyhow::Result<()> {
    let tor_path = which::which(tor_exe_name())?;
    let mut data_path = std::env::temp_dir();
    data_path.push("test_onion_service");

    let mut tor = TorManager::new(&tor_path, &data_path)?;

    // for 30secs for bootstrap
    tor.bootstrap()?;

    let mut bootstrap_complete = false;
    while !bootstrap_complete {
        for event in tor.update()?.iter() {
            match event {
                Event::BootstrapStatus {
                    progress,
                    tag,
                    summary,
                } => println!(
                    "BootstrapStatus: {{ progress: {}, tag: {}, summary: '{}' }}",
                    progress, tag, summary
                ),
                Event::BootstrapComplete => {
                    println!("Bootstrap Complete!");
                    bootstrap_complete = true;
                }
                Event::LogReceived { line } => {
                    println!("--- {}", line);
                }
                _ => {}
            }
        }
    }

    // vanilla V3 onion service
    {
        // create an onion service for this test
        let private_key = Ed25519PrivateKey::generate();

        println!("Starting and listening to onion service");
        const VIRT_PORT: u16 = 42069u16;
        let listener = tor.listener(&private_key, VIRT_PORT, None)?;

        let mut onion_published = false;
        while !onion_published {
            for event in tor.update()?.iter() {
                match event {
                    Event::LogReceived { line } => {
                        println!("--- {}", line);
                    }
                    Event::OnionServicePublished { service_id } => {
                        let expected_service_id = V3OnionServiceId::from_private_key(&private_key);
                        if expected_service_id == *service_id {
                            println!("Onion Service {} published", service_id.to_string());
                            onion_published = true;
                        }
                    }
                    _ => {}
                }
            }
        }

        const MESSAGE: &str = "Hello World!";

        {
            let service_id = V3OnionServiceId::from_private_key(&private_key);

            println!("Connecting to onion service");
            let mut client = tor.connect(&service_id, VIRT_PORT, None)?;
            println!("Client writing message: '{}'", MESSAGE);
            client.write_all(MESSAGE.as_bytes())?;
            client.flush()?;
            println!("End of client scope");
        }

        if let Some(mut server) = listener.accept()? {
            println!("Server reading message");
            let mut buffer = Vec::new();
            server.read_to_end(&mut buffer)?;
            let msg = String::from_utf8(buffer)?;

            assert!(MESSAGE == msg);
            println!("Message received: '{}'", msg);
        } else {
            panic!("no listener");
        }
    }

    // authenticated onion service
    {
        // create an onion service for this test
        let private_key = Ed25519PrivateKey::generate();

        let private_auth_key = X25519PrivateKey::generate();
        let public_auth_key = X25519PublicKey::from_private_key(&private_auth_key);

        println!("Starting and listening to authenticated onion service");
        const VIRT_PORT: u16 = 42069u16;
        let listener = tor.listener(&private_key, VIRT_PORT, Some(&[public_auth_key]))?;

        let mut onion_published = false;
        while !onion_published {
            for event in tor.update()?.iter() {
                match event {
                    Event::LogReceived { line } => {
                        println!("--- {}", line);
                    }
                    Event::OnionServicePublished { service_id } => {
                        let expected_service_id = V3OnionServiceId::from_private_key(&private_key);
                        if expected_service_id == *service_id {
                            println!(
                                "Authenticated Onion Service {} published",
                                service_id.to_string()
                            );
                            onion_published = true;
                        }
                    }
                    _ => {}
                }
            }
        }

        const MESSAGE: &str = "Hello World!";

        {
            let service_id = V3OnionServiceId::from_private_key(&private_key);

            println!("Connecting to onion service (should fail)");
            assert!(
                tor.connect(&service_id, VIRT_PORT, None).is_err(),
                "should not able to connect to an authenticated onion service without auth key"
            );

            println!("Add auth key for onion service");
            tor.add_client_auth(&service_id, &private_auth_key)?;

            println!("Connecting to onion service with authentication");
            let mut client = tor.connect(&service_id, VIRT_PORT, None)?;

            println!("Client writing message: '{}'", MESSAGE);
            client.write_all(MESSAGE.as_bytes())?;
            client.flush()?;
            println!("End of client scope");

            println!("Remove auth key for onion service");
            tor.remove_client_auth(&service_id)?;
        }

        if let Some(mut server) = listener.accept()? {
            println!("Server reading message");
            let mut buffer = Vec::new();
            server.read_to_end(&mut buffer)?;
            let msg = String::from_utf8(buffer)?;

            assert!(MESSAGE == msg);
            println!("Message received: '{}'", msg);
        } else {
            panic!("no listener");
        }
    }
    Ok(())
}
