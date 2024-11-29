// std
use std::str::FromStr;

// internal crates
use tor_interface::tor_provider::*;

//
// Misc Utils
//

#[test]
fn test_tor_provider_target_addr() -> anyhow::Result<()> {
    let valid_ip_addr: &[&str] = &[
        "192.168.1.1:80",
        "10.0.0.1:443",
        "172.16.0.1:8080",
        "8.8.8.8:53",
        "255.255.255.255:65535",
        "0.0.0.0:22",
        "192.168.0.254:21",
        "127.0.0.1:3306",
        "1.1.1.1:123",
        "224.0.0.1:554",
        "169.254.0.1:179",
        "203.0.113.1:80",
        "198.51.100.1:443",
        "100.64.0.1:8080",
        "192.0.2.1:53",
        "192.88.99.1:22",
        "192.0.0.1:21",
        "240.0.0.1:3306",
        "198.18.0.1:123",
        "233.252.0.1:554",
        "[2001:0db8:85a3:0000:0000:8a2e:0370:7334]:80",
        "[2001:db8:85a3::8a2e:370:7334]:443",
        "[::1]:8080",
        "[::ffff:192.168.1.1]:53",
        "[2001:0db8::1]:22",
        "[fe80::1ff:fe23:4567:890a]:21",
        "[2001:db8::1:0:0:1]:3306",
        "[2001:0db8:0000:0042:0000:8a2e:0370:7334]:123",
        "[ff02::1]:554",
        "[fe80::abcd:ef01:2345:6789]:179",
        "[2001:0db8:85a3:0000:0000:8a2e:0370:7334]:80",
        "[2001:db8:85a3::8a2e:370:7334]:443",
        "[::1]:8080",
        "[::ffff:c0a8:101]:53",
        "[2001:db8::1:0:0:1]:22",
        "[fe80::1ff:fe23:4567:890a]:21",
        "[2001:db8:0000:0042:0000:8a2e:0370:7334]:3306",
        "[ff02::1]:123",
        "[fe80::abcd:ef01:2345:6789]:554",
        "[2001:db8::1]:179",
    ];

    for target_addr_str in valid_ip_addr {
        match TargetAddr::from_str(target_addr_str) {
            Ok(TargetAddr::Socket(socket_addr)) => println!("{} => {}", target_addr_str, socket_addr),
            Ok(TargetAddr::OnionService(onion_addr)) => panic!(
                "unexpected conversion: {} => OnionService({})",
                target_addr_str, onion_addr
            ),
            Ok(TargetAddr::Domain(domain_addr)) => panic!(
                "unexpected conversion: {} => DomainAddr({})",
                target_addr_str, domain_addr
            ),
            Err(err) => Err(err)?,
        }
    }

    let valid_onion_addr: &[&str] = &[
        "6l62fw7tqctlu5fesdqukvpoxezkaxbzllrafa2ve6ewuhzphxczsjyd.onion:65535",
        "6L62FW7TQCTLU5FESDQUKVPOXEZKAXBZLLRAFA2VE6EWUHZPHXCZSJYD.onion:1",
    ];

    for target_addr_str in valid_onion_addr {
        match TargetAddr::from_str(target_addr_str) {
            Ok(TargetAddr::Socket(socket_addr)) => panic!(
                "unexpected conversion: {} => Ip({})",
                target_addr_str, socket_addr
            ),
            Ok(TargetAddr::OnionService(onion_addr)) => {
                println!("{} => {}", target_addr_str, onion_addr)
            }
            Ok(TargetAddr::Domain(domain_addr)) => panic!(
                "unexpected conversion: {} => DomainAddr({})",
                target_addr_str, domain_addr
            ),
            Err(err) => Err(err)?,
        }
    }

    let valid_domain_addr: &[&str] = &[
        "example.com:80",
        "subdomain.example.com:443",
        "xn--e1afmkfd.xn--p1ai:8080",       // domain in Punycode for "пример.рф"
        "xn--fsqu00a.xn--0zwm56d:53",       // domain in Punycode for "例子.测试"
        "münich.com:22",                    // domain with UTF-8 characters
        "xn--mnich-kva.com:21",             // Punycode for "münich.com"
        "exämple.com:3306",                 // domain with UTF-8 characters
        "xn--exmple-cua.com:123",           // Punycode for "exämple.com"
        "例子.com:554",                      // domain with UTF-8 characters
        "xn--fsqu00a.com:179",              // Punycode for "例子.com"
        "täst.de:80",                       // domain with UTF-8 characters
        "xn--tst-qla.de:443",               // Punycode for "täst.de"
        "xn--fiqs8s:80",                    // Punycode for "中国"
        "xn--wgbh1c:8080",                  // Punycode for "مصر"
        "münster.de:22",                    // domain with UTF-8 characters
        "xn--mnster-3ya.de:21",             // Punycode for "münster.de"
        "bücher.com:3306",                  // domain with UTF-8 characters
        "xn--bcher-kva.com:123",            // Punycode for "bücher.com"
        "xn--vermgensberatung-pwb.com:554", // Punycode for "vermögensberatung.com"
        // Max Length
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.ccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc.ddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd:65535"
    ];

    for target_addr_str in valid_domain_addr {
        match TargetAddr::from_str(target_addr_str) {
            Ok(TargetAddr::Socket(socket_addr)) => panic!(
                "unexpected conversion: {} => SocketAddr({})",
                target_addr_str, socket_addr
            ),
            Ok(TargetAddr::OnionService(onion_addr)) => panic!(
                "unexpected conversion: {} => OnionService({})",
                target_addr_str, onion_addr
            ),
            Ok(TargetAddr::Domain(domain_addr)) => {
                println!("{} => {}", target_addr_str, domain_addr)
            }
            Err(err) => Err(err)?,
        }
    }

    let invalid_target_addr: &[&str] = &[
        // ipv4-ish
        "192.168.1.1:99999", // Port number out of range
        "192.168.1.1:abc",   // Invalid port number
        "192.168.1.1:",      // Missing port number
        "192.168.1.1: 80",   // Space in port number
        "192.168.1.1:80a",   // Non-numeric characters in port number
        // ipv6-ish
        "[2001:db8:::1]:80",                            // Triple colons
        "[2001:db8:85a3::8a2e:370:7334:1234::abcd]:80", // Too many groups
        "[2001:db8:85a3::8a2e:370g:7334]:80",           // Invalid character in group
        "[2001:db8:85a3::8a2e:370:7334]:99999",         // Port number out of range
        "[2001:db8:85a3:8a2e:370:7334]:80",             // Missing double colons
        "[::12345]:80",                                 // Excessive leading zeroes
        "[2001:db8:85a3::8a2e:370:7334:]:80",           // Trailing colon
        "[2001:db8:85a3::8a2e:370:7334]",               // Missing port number
        "2001:db8:85a3::8a2e:370:7334:80",              // Missing square brackets
        "[2001:db8:85a3::8a2e:370:7334]: 80",           // Space in port number
        "[2001:db8:85a3::8a2e:370:7334]:80a",           // Non-numeric characters in port number
        // onion service-ish
        "6l62fw7tqctlu5fesdqukvpoxezkaxbzllrafa2ve6ewuhzphxczsjyd234567.onion:80", // Too long for v3
        "6l62fw7tqctlu5fesdqukvpoxezkaxbzllrafa2ve6ewuhzphxcz.onion:443", // Too short for v3
        "6l62fw7tqctlu5fesdqukvpoxezkaxbzllrafa2ve6ewuhzphxczsjyd.onion:99999", // Port number out of range
        "abcdefghijklmnopqrstuvwxyz0123456789abcdefghijklmnopqrst.onion:21", // Invalid characters
        "6l62fw7tqctlu5fesdqukvpoxezkaxbzllrafa2ve6ewuhzphxczsjyd.onion:abc", // Invalid port number
        "6l62fw7tqctlu5fesdqukvpoxezkaxbzllrafa2ve6ewuhzphxczsjyd.onion: 80", // Space in port number
        "6l62fw7tqctlu5fesdqukvpoxezkaxbzllrafa2ve6ewuhzphxczsjyd.onion:80a", // Non-numeric characters in port number
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.onion:80",  // Invalid service id
        // domain-ish
        "example..com:80",        // Double dots
        "exa mple.com:53",        // Space in domain
        "example.com:99999",      // Port number out of range
        "exaample.com:abc",       // Invalid port number
        "exaample.com:",          // Missing port number
        "exaample.com: 80",       // Space in port number
        "ex@mple.com:80",         // Special character in domain
        "example.com:80a",        // Non-numeric characters in port number
        "exämple..com:80",        // UTF-8 with double dot
        "xn--exmple-cua.com: 80", // Punycode with space in port number
        "xn--exmple-cua.com:80a", // Punycode with non-numeric port
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.com:65535", // Label too long
    ];

    for target_addr_str in invalid_target_addr {
        match TargetAddr::from_str(target_addr_str) {
            Ok(TargetAddr::Socket(socket_addr)) => panic!(
                "unexpected conversion: {} => SocketAddr({})",
                target_addr_str, socket_addr
            ),
            Ok(TargetAddr::OnionService(onion_addr)) => panic!(
                "unexpected conversion: {} => OnionService({})",
                target_addr_str, onion_addr
            ),
            Ok(TargetAddr::Domain(domain_addr)) => panic!(
                "unexpected conversion: {} => DomainAddr({})",
                target_addr_str, domain_addr
            ),
            Err(_) => (),
        }
    }

    Ok(())
}
