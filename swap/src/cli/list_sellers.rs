use crate::network::quote::BidQuote;
use libp2p::{Multiaddr, PeerId};
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use typeshare::typeshare;

// TODO: Move these types into swap-p2p?
#[serde_as]
#[typeshare]
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct QuoteWithAddress {
    /// The multiaddr of the seller (at which we were able to connect to and get the quote from)
    #[serde_as(as = "DisplayFromStr")]
    #[typeshare(serialized_as = "string")]
    pub multiaddr: Multiaddr,

    /// The peer id of the seller
    #[typeshare(serialized_as = "string")]
    pub peer_id: PeerId,

    /// The quote of the seller
    pub quote: BidQuote,

    /// The version of the seller's agent
    #[serde_as(as = "Option<DisplayFromStr>")]
    #[typeshare(serialized_as = "string")]
    pub version: Option<Version>,
}
