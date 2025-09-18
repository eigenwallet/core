///! This meta module describes **how to run** containers
///
/// Currently this only includes which flags we need to pass to the binaries
use crate::compose::{Flag, IntoFlag};

pub mod bitcoind {
    use super::*;

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
}

pub mod monerod {
    use super::*;

    impl IntoFlag for monero::Network {
        /// This is documented here:
        /// https://docs.getmonero.org/interacting/monerod-reference/#pick-monero-network-blockchain
        fn to_flag(self) -> Flag {
            Flag(match self {
                monero::Network::Mainnet => None,
                monero::Network::Stagenet => Some("--stagenet".to_string()),
                monero::Network::Testnet => Some("--testnet".to_string()),
            })
        }

        fn to_display(self) -> &'static str {
            match self {
                monero::Network::Mainnet => "mainnet",
                monero::Network::Stagenet => "stagenet",
                monero::Network::Testnet => "testnet",
            }
        }
    }
}

pub mod electrs {
    use super::*;
    use crate::flag;

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
                bitcoin::Network::Bitcoin => flag!("--network=bitcoin"),
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
}

pub mod asb {
    use super::*;
    use crate::{compose::Flag, flag};

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
                // Mainnet is the default for the asb
                (monero::Network::Mainnet, bitcoin::Network::Bitcoin) => Flag(None),
                // Testnet requires the --testnet flag
                (monero::Network::Stagenet, bitcoin::Network::Testnet) => flag!("--testnet"),
                _ => panic!(
                    "Only either Mainnet Bitcoin & Mainnet Monero or Testnet Bitcoin & Stagenet Monero are supported"
                ),
            }
        }

        fn to_display(self) -> &'static str {
            match self.0 {
                (monero::Network::Mainnet, bitcoin::Network::Bitcoin) => "mainnet",
                (monero::Network::Stagenet, bitcoin::Network::Testnet) => "testnet",
                _ => panic!(
                    "Only either Mainnet Bitcoin & Mainnet Monero or Testnet Bitcoin & Stagenet Monero are supported"
                ),
            }
        }
    }
}
