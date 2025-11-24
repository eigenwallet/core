use crate::network::quote::BidQuote;
use libp2p::{Multiaddr, PeerId};
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use typeshare::typeshare;


#[serde_as]
#[typeshare]
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Hash, Clone, Ord, PartialOrd)]
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

#[typeshare]
#[derive(Debug, Serialize, PartialEq, Eq, Hash, Clone, Ord, PartialOrd)]
pub struct UnreachableSeller {
    /// The peer id of the seller
    #[typeshare(serialized_as = "string")]
    pub peer_id: PeerId,
}

#[typeshare]
#[derive(Debug, Serialize, PartialEq, Eq, Hash, Clone, Ord, PartialOrd)]
#[serde(tag = "type", content = "content")]
pub enum SellerStatus {
    Online(QuoteWithAddress),
    Unreachable(UnreachableSeller),
}
