use crate::{
    compose::{Flag, IntoFlag},
    flag,
};

/// Wrapper around a Bitcoin network for Electrs
/// Electrs needs a different network flag than bitcoind
pub struct Network(bitcoin::Network);

#[allow(non_upper_case_globals)]
impl Network {
    pub const Mainnet: Self = Self(bitcoin::Network::Bitcoin);
    pub const Testnet: Self = Self(bitcoin::Network::Testnet);
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
