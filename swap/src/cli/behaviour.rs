use crate::network::rendezvous::XmrBtcNamespace;
use crate::network::swap_setup::bob;
use crate::network::{
    cooperative_xmr_redeem_after_punish, encrypted_signature, quote, redial, transfer_proof,
};
use anyhow::Result;
use bitcoin_wallet::BitcoinWallet;
use libp2p::swarm::NetworkBehaviour;
use libp2p::{identify, identity, ping};
use std::sync::Arc;
use std::time::Duration;
use swap_env::env;
pub use swap_p2p::out_event::bob::OutEvent;

const PROTOCOL_VERSION: &str = "/comit/xmr/btc/1.0.0";

const INITIAL_REDIAL_INTERVAL: Duration = Duration::from_secs(1);
const MAX_REDIAL_INTERVAL: Duration = Duration::from_secs(30);

/// A `NetworkBehaviour` that represents an XMR/BTC swap node as Bob.
#[derive(NetworkBehaviour)]
#[behaviour(to_swarm = "OutEvent")]
#[allow(missing_debug_implementations)]
pub struct Behaviour {
    pub quote: quote::Behaviour,
    pub swap_setup: bob::Behaviour,
    pub transfer_proof: transfer_proof::Behaviour,
    pub cooperative_xmr_redeem: cooperative_xmr_redeem_after_punish::Behaviour,
    pub encrypted_signature: encrypted_signature::Behaviour,
    pub redial: redial::Behaviour,
    pub identify: identify::Behaviour,

    /// Ping behaviour that ensures that the underlying network connection is
    /// still alive. If the ping fails a connection close event will be
    /// emitted that is picked up as swarm event.
    ping: ping::Behaviour,
}

impl Behaviour {
    pub fn new(
        env_config: env::Config,
        bitcoin_wallet: Arc<dyn BitcoinWallet>,
        identify_params: (identity::Keypair, XmrBtcNamespace),
    ) -> Self {
        let agentVersion = format!("cli/{} ({})", env!("CARGO_PKG_VERSION"), identify_params.1);

        let identifyConfig =
            identify::Config::new(PROTOCOL_VERSION.to_string(), identify_params.0.public())
                .with_agent_version(agentVersion);

        let pingConfig = ping::Config::new().with_timeout(Duration::from_secs(60));

        Self {
            quote: quote::cli(),
            swap_setup: bob::Behaviour::new(env_config, bitcoin_wallet),
            transfer_proof: transfer_proof::bob(),
            encrypted_signature: encrypted_signature::bob(),
            cooperative_xmr_redeem: cooperative_xmr_redeem_after_punish::bob(),
            redial: redial::Behaviour::new(INITIAL_REDIAL_INTERVAL, MAX_REDIAL_INTERVAL),
            ping: ping::Behaviour::new(pingConfig),
            identify: identify::Behaviour::new(identifyConfig),
        }
    }
}
