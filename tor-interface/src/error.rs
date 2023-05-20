#[derive(thiserror::Error, Debug)]
pub enum TorCryptoError {
    #[error("{0}")]
    ParseError(String),
    #[error("{0}")]
    ConversionError(String),
}

#[derive(thiserror::Error, Debug)]
pub enum TorProcessError {
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

#[derive(thiserror::Error, Debug)]
pub enum ControlStreamError {
    #[error("control stream read timeout must not be zero")]
    ReadTimeoutZero(),

    #[error("could not connect to control port")]
    CreationFailed(#[source] std::io::Error),

    #[error("configure control port socket failed")]
    ConfigurationFailed(#[source] std::io::Error),

    #[error("control port parsing regex creation failed")]
    ParsingRegexCreationFailed(#[source] regex::Error),

    #[error("control port stream read failure")]
    ReadFailed(#[source] std::io::Error),

    #[error("control port stream closed by remote")]
    ClosedByRemote(),

    #[error("received control port response invalid utf8")]
    InvalidResponse(#[source] std::str::Utf8Error),

    #[error("failed to parse control port reply: {0}")]
    ReplyParseFailed(String),

    #[error("control port stream write failure")]
    WriteFailed(#[source] std::io::Error),
}

#[derive(thiserror::Error, Debug)]
pub enum TorVersionError {
    #[error("{}", .0)]
    ParseError(String),
}

#[derive(thiserror::Error, Debug)]
pub enum TorControllerError {
    #[error("response regex creation failed")]
    ParsingRegexCreationFailed(#[source] regex::Error),

    #[error("control stream read reply failed")]
    ReadReplyFailed(#[source] ControlStreamError),

    #[error("unexpected synchronous reply recieved")]
    UnexpectedSynchonousReplyReceived(),

    #[error("control stream write command failed")]
    WriteCommandFailed(#[source] ControlStreamError),

    #[error("invalid command arguments: {0}")]
    InvalidCommandArguments(String),

    #[error("command failed: {0} {}", .1.join("\n"))]
    CommandReturnedError(u32, Vec<String>),

    #[error("failed to parse command reply: {0}")]
    CommandReplyParseFailed(String),

    #[error("failed to parse received tor version")]
    TorVersionParseFailed(#[source] TorVersionError),
}

#[derive(thiserror::Error, Debug)]
pub enum TorManagerError {
    #[error("failed to create TorProcess object")]
    TorProcessCreationFailed(#[source] TorProcessError),

    #[error("failed to create ControlStream object")]
    ControlStreamCreationFailed(#[source] ControlStreamError),

    #[error("failed to create TorController object")]
    TorControllerCreationFailed(#[source] TorControllerError),

    #[error("failed to authenticate with the tor process")]
    TorProcessAuthenticationFailed(#[source] TorControllerError),

    #[error("failed to determine the tor process version")]
    GetInfoVersionFailed(#[source] TorControllerError),

    #[error("tor process version to old; found {0} but must be at least {1}")]
    TorProcessTooOld(String, String),

    #[error("failed to register for STATUS_CLIENT and HS_DESC events")]
    SetEventsFailed(#[source] TorControllerError),

    #[error("failed to delete unused onion service")]
    DelOnionFailed(#[source] TorControllerError),

    #[error("failed waiting for async events")]
    WaitAsyncEventsFailed(#[source] TorControllerError),

    #[error("failed to begin bootstrap")]
    SetConfDisableNetwork0Failed(#[source] TorControllerError),

    #[error("failed to add client auth for onion service")]
    OnionClientAuthAddFailed(#[source] TorControllerError),

    #[error("failed to remove client auth from onion service")]
    OnionClientAuthRemoveFailed(#[source] TorControllerError),

    #[error("failed to get socks listener")]
    GetInfoNetListenersSocksFailed(#[source] TorControllerError),

    #[error("no socks listeners available to connect through")]
    NoSocksListenersFound(),

    #[error("unable to connect to socks listener")]
    Socks5ConnectionFailed(#[source] std::io::Error),

    #[error("unable to bind TCP listener")]
    TcpListenerBindFailed(#[source] std::io::Error),

    #[error("unable to get TCP listener's local address")]
    TcpListenerLocalAddrFailed(#[source] std::io::Error),

    #[error("faild to create onion service")]
    AddOnionFailed(#[source] TorControllerError),
}
