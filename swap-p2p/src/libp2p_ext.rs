use libp2p::multiaddr::Protocol;
use libp2p::{Multiaddr, PeerId};
use std::collections::HashMap;

pub trait MultiAddrExt {
    fn extract_peer_id(&self) -> Option<PeerId>;
    fn split_peer_id(&self) -> Option<(PeerId, Multiaddr)>;
    fn is_local(&self) -> bool;
}

impl MultiAddrExt for Multiaddr {
    fn extract_peer_id(&self) -> Option<PeerId> {
        match self.iter().last()? {
            Protocol::P2p(peer_id) => Some(peer_id),
            _ => None,
        }
    }

    // Takes a peer id like /ip4/192.168.178.64/tcp/9939/p2p/12D3KooWQsqsCyJ9ae1YEAJZAfoVdVFZdDdUq3yvZ92btq7hSv9f
    // and returns the peer id and the original address *with* the peer id
    fn split_peer_id(&self) -> Option<(PeerId, Multiaddr)> {
        let peer_id = self.extract_peer_id()?;
        let address = self.clone();
        Some((peer_id, address))
    }

    // Returns true if the multi address contains a local address which should not be advertised to the global internet
    fn is_local(&self) -> bool {
        self.iter().any(|p| match p {
            Protocol::Ip4(addr) => {
                addr.is_private()
                    || addr.is_loopback()
                    || addr.is_link_local()
                    || addr.is_unspecified()
            }
            Protocol::Ip6(addr) => {
                addr.is_unique_local()
                    || addr.is_loopback()
                    || addr.is_unicast_link_local()
                    || addr.is_unspecified()
            }
            _ => false,
        })
    }
}

pub trait MultiAddrVecExt {
    /// Takes multiaddresses where each multiaddress contains a peer id
    /// and returns a vector of peer ids and their respective addresses
    fn extract_peer_addresses(&self) -> Vec<(PeerId, Vec<Multiaddr>)>;
}

impl MultiAddrVecExt for Vec<String> {
    fn extract_peer_addresses(&self) -> Vec<(PeerId, Vec<Multiaddr>)> {
        let addresses = self
            .iter()
            .filter_map(|addr| addr.parse::<Multiaddr>().ok())
            .collect::<Vec<_>>();

        parse_strings_to_multiaddresses(&addresses)
    }
}

impl MultiAddrVecExt for Vec<Multiaddr> {
    fn extract_peer_addresses(&self) -> Vec<(PeerId, Vec<Multiaddr>)> {
        parse_strings_to_multiaddresses(self)
    }
}

pub fn parse_strings_to_multiaddresses(addresses: &[Multiaddr]) -> Vec<(PeerId, Vec<Multiaddr>)> {
    let mut map: HashMap<PeerId, Vec<Multiaddr>> = HashMap::new();

    for addr in addresses {
        if let Some(peer_id) = addr.extract_peer_id() {
            map.entry(peer_id).or_default().push(addr.clone());
        }
    }

    map.into_iter().collect()
}
