use crate::{
    compose::{Flag, IntoFlag},
    flag,
};

/// Wrapper around the network used for ASB
/// There are only two combinations of networks that are supported:
/// - Mainnet Bitcoin & Mainnet Monero
/// - Testnet Bitcoin & Stagenet Monero
pub struct Network((monero::Network, bitcoin::Network));

impl Network {
    pub fn new(monero: monero::Network, bitcoin: bitcoin::Network) -> Self {
        Self((monero, bitcoin))
    }
}

impl IntoFlag for Network {
    fn to_flag(self) -> Flag {
        match self.0 {
            (monero::Network::Mainnet, bitcoin::Network::Bitcoin) => flag!("--mainnet"),
            (monero::Network::Stagenet, bitcoin::Network::Testnet) => flag!("--testnet"),
            _ => panic!("Only either Mainnet Bitcoin & Mainnet Monero or Testnet Bitcoin & Stagenet Monero are supported"),
        }
    }

    fn to_display(self) -> &'static str {
        match self.0 {
            (monero::Network::Mainnet, bitcoin::Network::Bitcoin) => "mainnet",
            (monero::Network::Stagenet, bitcoin::Network::Testnet) => "testnet",
            _ => panic!("Only either Mainnet Bitcoin & Mainnet Monero or Testnet Bitcoin & Stagenet Monero are supported"),
        }
    }
}
