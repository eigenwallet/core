use crate::{
    compose::{Flag, IntoFlag},
    flag,
};

/// Wrapper around a Bitcoin network for Electrs
/// Electrs needs a different network flag than bitcoind
#[derive(Clone)]
pub struct Network(bitcoin::Network);

impl Network {
    pub fn new(bitcoin: bitcoin::Network) -> Self {
        Self(bitcoin)
    }
}

impl IntoFlag for Network {
    fn to_flag(self) -> Flag {
        match self.0 {
            bitcoin::Network::Bitcoin => flag!("--network=mainnet"),
            bitcoin::Network::Testnet => flag!("--network=testnet"),
            _ => panic!("Only Mainnet and Testnet are supported"),
        }
    }

    fn to_display(self) -> &'static str {
        match self.0 {
            bitcoin::Network::Bitcoin => "mainnet",
            bitcoin::Network::Testnet => "testnet",
            _ => panic!("Only Mainnet and Testnet are supported"),
        }
    }
}
