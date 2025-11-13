use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::FmtSubscriber;

pub fn init_tracing(level: LevelFilter, json_format: bool, no_timestamp: bool) {
    if level == LevelFilter::OFF {
        return;
    }

    let is_terminal = atty::is(atty::Stream::Stderr);

    let builder = FmtSubscriber::builder()
        .with_env_filter(build_event_filter_str(&[
            (&[env!("CARGO_CRATE_NAME")], level),
            (LIBP2P_CRATES, level),
        ]))
        .with_writer(std::io::stderr)
        .with_ansi(is_terminal)
        .with_target(false);

    if json_format {
        builder.json().init();
        return;
    }

    if no_timestamp {
        builder.without_time().init();
        return;
    }
    builder.init();
}

fn build_event_filter_str(crates_with_filters: &[(&[&str], LevelFilter)]) -> String {
    crates_with_filters
        .iter()
        .flat_map(|(crates, level)| {
            crates
                .iter()
                .map(move |crate_name| format!("{}={}", crate_name, level))
        })
        .collect::<Vec<_>>()
        .join(",")
}

const LIBP2P_CRATES: &[&str] = &[
    "libp2p",
    "libp2p_allow_block_list",
    "libp2p_connection_limits",
    "libp2p_core",
    "libp2p_dns",
    "libp2p_identity",
    "libp2p_noise",
    "libp2p_ping",
    "libp2p_rendezvous",
    "libp2p_request_response",
    "libp2p_swarm",
    "libp2p_tcp",
    "libp2p_tls",
    "libp2p_tor",
    "libp2p_websocket",
    "libp2p_yamux",
];
