use std::io::{self, IsTerminal};
use std::path::Path;
use std::str::FromStr;

use anyhow::Result;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::filter::{Directive, LevelFilter};
use tracing_subscriber::fmt::time::UtcTime;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter, Layer};

use crate::cli::api::tauri_bindings::{TauriEmitter, TauriHandle, TauriLogEvent};

const TOR_CRATES: &[&str] = &["arti", "arti_client"];

#[allow(dead_code)] // might be useful later
const TOR_SUBCRATES: &[&str] = &[
    "arti_fork",
    "tor_api2",
    "tor_async_utils",
    "tor_basic_utils",
    "tor_bytes",
    "tor_cell",
    "tor_cert",
    "tor_chanmgr",
    "tor_checkable",
    "tor_circmgr",
    "tor_config",
    "tor_config_path",
    "tor_consdiff",
    "tor_dirclient",
    "tor_dirmgr",
    "tor_error",
    "tor_general_addr",
    "tor_guardmgr",
    "tor_hsclient",
    "tor_hscrypto",
    "tor_hsservice",
    "tor_key_forge",
    "tor_keymgr",
    "tor_linkspec",
    "tor_llcrypto",
    "tor_log_ratelim",
    "tor_memquota",
    "tor_netdir",
    "tor_netdoc",
    "tor_persist",
    "tor_proto",
    "tor_protover",
    "tor_relay_selection",
    "tor_rtcompat",
    "tor_rtmock",
    "tor_socksproto",
    "tor_units",
];

const LIBP2P_CRATES: &[&str] = &[
    "libp2p",
    "libp2p_swarm",
    "libp2p_core",
    "libp2p_tcp",
    "libp2p_noise",
    "libp2p_tor",
    "libp2p_core::transport",
    "libp2p_core::transport::choice",
    "libp2p_core::transport::dummy",
    "libp2p_swarm::connection",
    "libp2p_swarm::dial",
    "libp2p_tcp::transport",
    "libp2p_noise::protocol",
    "libp2p_identify",
    "libp2p_ping",
    "libp2p_request_response",
    "libp2p_kad",
    "libp2p_dns",
    "libp2p_yamux",
    "libp2p_quic",
    "libp2p_websocket",
    "libp2p_relay",
    "libp2p_autonat",
    "libp2p_mdns",
    "libp2p_gossipsub",
    "libp2p_rendezvous",
    "libp2p_dcutr",
];

const OUR_CRATES: &[&str] = &[
    "swap",
    "swap_p2p",
    "asb",
    "swap_env",
    "swap_fs",
    "swap_serde",
    "unstoppableswap_gui_rs",
];

const MONERO_WALLET_CRATES: &[&str] = &[
    "monero_sys",
    "monero_cpp",
    "monero_rpc_pool",
];

macro_rules! json_rolling_layer {
    ($dir:expr, $prefix:expr, $env_filter:expr, $max_files:expr) => {{
        let appender: RollingFileAppender = RollingFileAppender::builder()
            .rotation(Rotation::HOURLY)
            .filename_prefix($prefix)
            .filename_suffix("log")
            .max_log_files($max_files)
            .build($dir)
            .expect("initializing rolling file appender failed");

        fmt::layer()
            .with_writer(appender)
            .with_ansi(false)
            .with_timer(UtcTime::rfc_3339())
            .with_target(false)
            .with_file(true)
            .with_line_number(true)
            .json()
            .with_filter($env_filter?)
    }};
}

/// Output formats for logging messages.
pub enum Format {
    /// Standard, human readable format.
    Raw,
    /// JSON, machine readable format.
    Json,
}

/// Initialize tracing and enable logging messages according to these options.
/// Besides printing to `stdout`, this will append to a log file.
/// Said file will contain JSON-formatted logs of all levels,
/// disregarding the arguments to this function. When `trace_stdout` is `true`,
/// all tracing logs are also emitted to stdout.
pub fn init(
    format: Format,
    dir: impl AsRef<Path>,
    tauri_handle: Option<TauriHandle>,
    trace_stdout: bool,
) -> Result<()> {
    // Write our crates to the general log file at DEBUG level
    let file_layer = {
        let file_appender: RollingFileAppender =
            tracing_appender::rolling::never(&dir, "swap-all.log");

        fmt::layer()
            .with_writer(file_appender)
            .with_ansi(false)
            .with_timer(UtcTime::rfc_3339())
            .with_target(false)
            .with_file(true)
            .with_line_number(true)
            .json()
            .with_filter(env_filter_with_all_crates(vec![(
                OUR_CRATES.to_vec(),
                LevelFilter::DEBUG,
            )])?)
    };

    // Write our crates to a verbose log file (tracing*.log)
    let tracing_file_layer = json_rolling_layer!(
        &dir,
        "tracing",
        env_filter_with_all_crates(vec![
            (OUR_CRATES.to_vec(), LevelFilter::TRACE),
            (LIBP2P_CRATES.to_vec(), LevelFilter::TRACE),
        ]),
        24
    );

    // Write Tor/arti to a verbose log file (tracing-tor*.log)
    let tor_file_layer = json_rolling_layer!(
        &dir,
        "tracing-tor",
        env_filter_with_all_crates(vec![(TOR_CRATES.to_vec(), LevelFilter::TRACE)]),
        24
    );

    // Write libp2p to a verbose log file (tracing-libp2p*.log)
    let libp2p_file_layer = json_rolling_layer!(
        &dir,
        "tracing-libp2p",
        env_filter_with_all_crates(vec![(LIBP2P_CRATES.to_vec(), LevelFilter::TRACE)]),
        24
    );

    // Write monero wallet crates to a verbose log file (tracing-monero-wallet*.log)
    let monero_wallet_file_layer = json_rolling_layer!(
        &dir,
        "tracing-monero-wallet",
        env_filter_with_all_crates(vec![(MONERO_WALLET_CRATES.to_vec(), LevelFilter::TRACE)]),
        24
    );

    // Layer for writing to the terminal
    let is_terminal = std::io::stderr().is_terminal();
    let terminal_layer = fmt::layer()
        .with_writer(std::io::stderr)
        .with_ansi(is_terminal)
        .with_timer(UtcTime::rfc_3339())
        .with_target(true)
        .with_file(true)
        .with_line_number(true);

    // Layer for writing to the Tauri guest. This will be displayed in the GUI.
    // Crates: All crates with libp2p at INFO+ level
    // Level: Passed in for our crates, INFO for libp2p
    let tauri_layer = fmt::layer()
        .with_writer(TauriWriter::new(tauri_handle))
        .with_ansi(false)
        .with_timer(UtcTime::rfc_3339())
        .with_target(true)
        .with_file(true)
        .with_line_number(true)
        .json()
        .with_filter(env_filter_with_all_crates(vec![
            (OUR_CRATES.to_vec(), LevelFilter::TRACE),
            (MONERO_WALLET_CRATES.to_vec(), LevelFilter::INFO),
            (LIBP2P_CRATES.to_vec(), LevelFilter::INFO),
            (TOR_CRATES.to_vec(), LevelFilter::INFO),
        ])?);

    // If trace_stdout is true, we log all messages to the terminal
    // Otherwise, we only log the bare minimum
    let terminal_layer_env_filter = match trace_stdout {
        true => env_filter_with_all_crates(vec![
            (OUR_CRATES.to_vec(), LevelFilter::DEBUG),
            (TOR_CRATES.to_vec(), LevelFilter::INFO),
            (LIBP2P_CRATES.to_vec(), LevelFilter::INFO),
        ])?,
        false => env_filter_with_all_crates(vec![(OUR_CRATES.to_vec(), LevelFilter::INFO)])?,
    };

    let final_terminal_layer = match format {
        Format::Json => terminal_layer
            .json()
            .with_filter(terminal_layer_env_filter)
            .boxed(),
        Format::Raw => terminal_layer
            .with_filter(terminal_layer_env_filter)
            .boxed(),
    };

    let subscriber = tracing_subscriber::registry()
        .with(file_layer)
        .with(tracing_file_layer)
        .with(tor_file_layer)
        .with(libp2p_file_layer)
        .with(monero_wallet_file_layer)
        .with(final_terminal_layer)
        .with(tauri_layer);

    subscriber.try_init()?;

    // Now we can use the tracing macros to log messages
    tracing::info!(
        logs_dir = %dir.as_ref().display(),
        "Initialized tracing. General logs go to swap-all.log; verbose logs: tracing*.log (ours), tracing-tor*.log (tor), tracing-libp2p*.log (libp2p)"
    );

    Ok(())
}

/// This function controls which crate's logs actually get logged and from which level, including all crate categories.
fn env_filter_with_all_crates(crates: Vec<(Vec<&str>, LevelFilter)>) -> Result<EnvFilter> {
    let mut filter = EnvFilter::from_default_env();

    // Add directives for each group of crates with their specified level filter
    for (crate_names, level_filter) in crates {
        for crate_name in crate_names {
            filter = filter.add_directive(Directive::from_str(&format!(
                "{}={}",
                crate_name, &level_filter
            ))?);
        }
    }

    Ok(filter)
}

/// A writer that forwards tracing log messages to the tauri guest.
#[derive(Clone)]
pub struct TauriWriter {
    tauri_handle: Option<TauriHandle>,
}

impl TauriWriter {
    /// Create a new Tauri writer that sends log messages to the tauri guest.
    pub fn new(tauri_handle: Option<TauriHandle>) -> Self {
        Self { tauri_handle }
    }
}

/// This is needed for tracing to accept this as a writer.
impl<'a> MakeWriter<'a> for TauriWriter {
    type Writer = TauriWriter;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// For every write issued by tracing we simply pass the string on as an event to the tauri guest.
impl std::io::Write for TauriWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // Since this function accepts bytes, we need to pass to utf8 first
        let owned_buf = buf.to_owned();
        let utf8_string = String::from_utf8(owned_buf)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;

        // Then send to tauri
        self.tauri_handle.emit_cli_log_event(TauriLogEvent {
            buffer: utf8_string,
        });

        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        // No-op, we don't need to flush anything
        Ok(())
    }
}