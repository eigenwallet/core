use crate::compose::{Flag, IntoFlag};

impl IntoFlag for bitcoin::Network {
    /// This is documented here:
    /// https://www.mankier.com/1/bitcoind
    fn to_flag(self) -> Flag {
        Flag(Some(match self {
            bitcoin::Network::Bitcoin => "-chain=main".to_string(),
            bitcoin::Network::Testnet => "-chain=test".to_string(),
            _ => panic!("Only Mainnet and Testnet are supported"),
        }))
    }

    fn to_display(self) -> &'static str {
        match self {
            bitcoin::Network::Bitcoin => "mainnet",
            bitcoin::Network::Testnet => "testnet",
            _ => panic!("Only Mainnet and Testnet are supported"),
        }
    }
}
