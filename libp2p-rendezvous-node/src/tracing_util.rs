use tracing::level_filters::LevelFilter;
use tracing_subscriber::FmtSubscriber;

pub fn init_tracing(level: LevelFilter) {
    if level == LevelFilter::OFF {
        return;
    }

    let is_terminal = atty::is(atty::Stream::Stderr);

    FmtSubscriber::builder()
        .with_env_filter(format!(
            "rendezvous_server={},\
             swap_p2p={},\
             libp2p={},
             libp2p_rendezvous={},\
             libp2p_swarm={},\
             libp2p_tor={}",
            level, level, level, level, level, level
        ))
        .with_writer(std::io::stderr)
        .with_ansi(is_terminal)
        .with_target(false)
        .init();
}
