// TODO: Move this to swap-p2p
use libp2p::multiaddr::Protocol;
use libp2p::{Multiaddr, PeerId};
use std::collections::HashMap;
use std::str::FromStr;

pub trait MultiAddrExt {
    fn extract_peer_id(&self) -> Option<PeerId>;
    fn split_peer_id(&self) -> Option<(PeerId, Multiaddr)>;
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
}

pub fn parse_strings_to_multiaddresses(addresses: &[String]) -> Vec<(PeerId, Vec<Multiaddr>)> {
    let mut map: HashMap<PeerId, Vec<Multiaddr>> = HashMap::new();

    for addr_str in addresses {
        if let Ok(multiaddr) = Multiaddr::from_str(addr_str) {
            if let Some(peer_id) = multiaddr.extract_peer_id() {
                map.entry(peer_id).or_default().push(multiaddr);
            }
        }
    }

    map.into_iter().collect()
}
