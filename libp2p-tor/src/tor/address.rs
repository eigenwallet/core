// Copyright 2022 Hannes Furmans
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.
use libp2p::{core::multiaddr::Protocol, Multiaddr};
use tor_interface::tor_provider::{OnionAddr, OnionAddrV3, TargetAddr, DomainAddr};
use tor_interface::tor_crypto::{V3OnionServiceId, Ed25519PublicKey};
use std::net::SocketAddr;

/// "Dangerously" extract a Tor address from the provided [`Multiaddr`].
///
/// Refer tor [arti_client::TorAddr](https://docs.rs/arti-client/latest/arti_client/struct.TorAddr.html) for details around the safety / privacy considerations.
pub fn dangerous_extract(multiaddr: &Multiaddr) -> Option<TargetAddr> {
    if let Some(tor_addr) = safe_extract(multiaddr) {
        return Some(tor_addr);
    }

    let mut protocols = multiaddr.into_iter();

    try_to_socket_addr(&protocols.next()?, &protocols.next()?)
}

/// "Safely" extract a Tor address from the provided [`Multiaddr`].
///
/// Refer tor [arti_client::TorAddr](https://docs.rs/arti-client/latest/arti_client/struct.TorAddr.html) for details around the safety / privacy considerations.
pub fn safe_extract(multiaddr: &Multiaddr) -> Option<TargetAddr> {
    let mut protocols = multiaddr.into_iter();

    let (dom, port) = (protocols.next()?, protocols.next());
    try_to_domain_and_port(&dom, &port)
}

fn try_to_domain_and_port<'a>(
    maybe_domain: &'a Protocol,
    maybe_port: &Option<Protocol>,
) -> Option<TargetAddr> {
    match (maybe_domain, maybe_port) {
        (
            Protocol::Dns(domain) | Protocol::Dns4(domain) | Protocol::Dns6(domain),
            Some(Protocol::Tcp(port)),
        ) => Some(TargetAddr::Domain(DomainAddr::try_from((domain.to_string(), *port)).ok()?.into())),
        (Protocol::Onion3(domain), _) =>
            Some(TargetAddr::OnionService(OnionAddr::V3(OnionAddrV3::new(V3OnionServiceId::from_public_key(&Ed25519PublicKey::from_raw(domain.hash()[..32].try_into().unwrap()).ok()?), domain.port())))),
        _ => None,
    }
}

fn try_to_socket_addr(maybe_ip: &Protocol, maybe_port: &Protocol) -> Option<TargetAddr> {
    match (maybe_ip, maybe_port) {
        (Protocol::Ip4(ip), Protocol::Tcp(port)) => Some(TargetAddr::Socket(SocketAddr::from((*ip, *port)))),
        (Protocol::Ip6(ip), Protocol::Tcp(port)) => Some(TargetAddr::Socket(SocketAddr::from((*ip, *port)))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tor_interface::tor_provider::TargetAddr;
    use std::str::FromStr;

    #[test]
    fn extract_correct_address_from_dns() {
        let addresses = [
            "/dns/ip.tld/tcp/10".parse().unwrap(),
            "/dns4/dns.ip4.tld/tcp/11".parse().unwrap(),
            "/dns6/dns.ip6.tld/tcp/12".parse().unwrap(),
            "/onion3/cebulka7uxchnbpvmqapg5pfos4ngaxglsktzvha7a5rigndghvadeyd:13".parse().unwrap(),
        ];

        let actual = addresses
            .iter()
            .filter_map(safe_extract)
            .collect::<Vec<_>>();

        assert_eq!(
            &[
                TargetAddr::from_str("ip.tld:10").unwrap(),
                TargetAddr::from_str("dns.ip4.tld:11").unwrap(),
                TargetAddr::from_str("dns.ip6.tld:12").unwrap(),
                TargetAddr::from_str("cebulka7uxchnbpvmqapg5pfos4ngaxglsktzvha7a5rigndghvadeyd.onion:13").unwrap(),
            ],
            actual.as_slice()
        );
    }

    #[test]
    fn extract_correct_address_from_ips() {
        let addresses = [
            "/ip4/127.0.0.1/tcp/10".parse().unwrap(),
            "/ip6/::1/tcp/10".parse().unwrap(),
        ];

        let actual = addresses
            .iter()
            .filter_map(dangerous_extract)
            .collect::<Vec<_>>();

        assert_eq!(
            &[
                TargetAddr::from_str("127.0.0.1:10").unwrap(),
                TargetAddr::from_str("[::1]:10").unwrap(),
            ],
            actual.as_slice()
        );
    }

    #[test]
    fn dangerous_extract_works_on_domains_too() {
        let addresses = [
            "/dns/ip.tld/tcp/10".parse().unwrap(),
            "/ip4/127.0.0.1/tcp/10".parse().unwrap(),
            "/ip6/::1/tcp/10".parse().unwrap(),
        ];

        let actual = addresses
            .iter()
            .filter_map(dangerous_extract)
            .collect::<Vec<_>>();

        assert_eq!(
            &[
                TargetAddr::from_str("ip.tld:10").unwrap(),
                TargetAddr::from_str("127.0.0.1:10").unwrap(),
                TargetAddr::from_str("[::1]:10").unwrap(),
            ],
            actual.as_slice()
        );
    }

    #[test]
    fn detect_incorrect_address() {
        let addresses = [
            "/tcp/10/udp/12".parse().unwrap(),
            "/dns/ip.tld/dns4/ip.tld/dns6/ip.tld".parse().unwrap(),
            "/tcp/10/ip4/1.1.1.1".parse().unwrap(),
        ];

        let all_correct = addresses.iter().map(safe_extract).all(|res| res.is_none());

        assert!(
            all_correct,
            "During the parsing of the faulty addresses, there was an incorrectness"
        );
    }
}
