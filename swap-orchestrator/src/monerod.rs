use crate::compose::{Flag, IntoFlag};

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
