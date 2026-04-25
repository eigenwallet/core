//! Shared SOCKS5 settings for the current process.
//!
//! Store the proxy address once during startup and reconnect after changes.
//! [`proxy_config`] includes per-subsystem credentials for stream isolation.
//! [`proxy_addr`] is for APIs that only accept `host:port`.
//! IPv4 only.

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddrV4, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

/// Default Tor SOCKS5 proxy address (standard Tor daemon port).
pub const DEFAULT_ADDR: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 9050);

const PROBE_TIMEOUT: Duration = Duration::from_millis(250);

static ENABLED: AtomicBool = AtomicBool::new(false);
/// Packed IPv4 address and port: `(ip as u64) << 16 | port`.
static ADDR: AtomicU64 = AtomicU64::new((0x7f00_0001_u64 << 16) | 9050);

fn pack(addr: SocketAddrV4) -> u64 {
    ((u32::from(*addr.ip()) as u64) << 16) | (addr.port() as u64)
}

fn unpack(value: u64) -> SocketAddrV4 {
    let ip = Ipv4Addr::from((value >> 16) as u32);
    let port = value as u16;
    SocketAddrV4::new(ip, port)
}

/// Returns the currently configured proxy address.
pub fn current_addr() -> SocketAddrV4 {
    unpack(ADDR.load(Ordering::Relaxed))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Subsystem {
    Http,
    Updater,
    Electrum,
    MoneroRpc,
    Bitcoin,
    Libp2p,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProxyConfig {
    pub addr: SocketAddrV4,
    /// Tor uses `(username, password)` as the isolation key.
    pub username: &'static str,
    pub password: &'static str,
}

impl ProxyConfig {
    pub fn url(self) -> String {
        format!("socks5h://{}:{}@{}", self.username, self.password, self.addr)
    }

    /// Build a `reqwest::Proxy` for this SOCKS5 endpoint.
    #[cfg(feature = "reqwest")]
    pub fn reqwest_proxy(self) -> reqwest::Result<reqwest::Proxy> {
        reqwest::Proxy::all(self.url())
    }

    /// Dial `target` through the SOCKS5 proxy.
    pub async fn connect<'a, T>(
        self,
        target: T,
    ) -> Result<tokio_socks::tcp::Socks5Stream<tokio::net::TcpStream>, tokio_socks::Error>
    where
        T: tokio_socks::IntoTargetAddr<'a>,
    {
        tokio_socks::tcp::Socks5Stream::connect_with_password(
            self.addr,
            target,
            self.username,
            self.password,
        )
        .await
    }
}

fn isolation_token(subsystem: Subsystem) -> &'static str {
    match subsystem {
        Subsystem::Http => "http",
        Subsystem::Updater => "updater",
        Subsystem::Electrum => "electrum",
        Subsystem::MoneroRpc => "monero",
        Subsystem::Bitcoin => "bitcoin",
        Subsystem::Libp2p => "libp2p",
    }
}

impl Subsystem {
    /// Build a SOCKS5 URL for `addr` without reading global state.
    pub fn proxy_url_for(self, addr: SocketAddrV4) -> String {
        let token = isolation_token(self);
        ProxyConfig {
            addr,
            username: token,
            password: token,
        }
        .url()
    }
}

/// Enable SOCKS5 routing on [`DEFAULT_ADDR`].
pub fn enable() {
    enable_with_addr(DEFAULT_ADDR);
}

/// Enable SOCKS5 routing on `addr`.
///
/// The `Release` store on `ENABLED` orders the preceding `ADDR` update.
pub fn enable_with_addr(addr: SocketAddrV4) {
    ADDR.store(pack(addr), Ordering::Relaxed);
    ENABLED.store(true, Ordering::Release);
    tracing::info!(proxy = %current_addr(), "System Tor SOCKS5 proxy enabled");
}

/// Disable SOCKS5 routing.
pub fn disable() {
    ENABLED.store(false, Ordering::Release);
    tracing::info!("System Tor SOCKS5 proxy disabled");
}

/// Return whether SOCKS5 routing is enabled.
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Acquire)
}

/// Return the subsystem-specific proxy settings, if enabled.
pub fn proxy_config(subsystem: Subsystem) -> Option<ProxyConfig> {
    let token = isolation_token(subsystem);

    is_enabled().then_some(ProxyConfig {
        addr: current_addr(),
        username: token,
        password: token,
    })
}

/// Return the proxy socket address, if enabled.
///
/// Use this only for APIs that accept `host:port` without SOCKS5 credentials.
pub fn proxy_addr() -> Option<SocketAddrV4> {
    is_enabled().then_some(current_addr())
}

/// Probe an IPv4 `ip:port` string and return whether a SOCKS5 server answers.
pub fn probe_addr_str(address: &str) -> bool {
    match address.parse::<SocketAddrV4>() {
        Ok(addr) => probe_addr(addr),
        Err(_) => false,
    }
}

/// Send a SOCKS5 greeting and require a SOCKS5 reply.
fn probe_addr(addr: SocketAddrV4) -> bool {
    let addr_str = addr.to_string();
    let mut stream = match TcpStream::connect_timeout(&addr.into(), PROBE_TIMEOUT) {
        Ok(s) => s,
        Err(error) => {
            tracing::debug!(
                proxy = %addr_str,
                %error,
                "System Tor SOCKS5 proxy unreachable",
            );
            return false;
        }
    };

    let _ = stream.set_read_timeout(Some(PROBE_TIMEOUT));
    let _ = stream.set_write_timeout(Some(PROBE_TIMEOUT));

    if stream.write_all(&[0x05, 0x01, 0x00]).is_err() {
        tracing::debug!(proxy = %addr_str, "Failed to send SOCKS5 greeting");
        return false;
    }

    let mut buf = [0u8; 2];
    if stream.read_exact(&mut buf).is_err() {
        tracing::debug!(proxy = %addr_str, "Failed to read SOCKS5 response");
        return false;
    }

    if buf != [0x05, 0x00] {
        tracing::debug!(
            proxy = %addr_str,
            response = ?buf,
            "Not a SOCKS5 server (unexpected handshake response)",
        );
        return false;
    }

    true
}

/// Return whether `host` should bypass the proxy.
///
/// Bare hostnames without a dot are not treated as local.
pub fn is_local_host(host: &str) -> bool {
    let lower = host.to_ascii_lowercase();

    if lower == "localhost" || lower.ends_with(".local") {
        return true;
    }

    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => ip.is_loopback() || ip.is_private() || ip.is_link_local(),
        Ok(IpAddr::V6(ip)) => {
            ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local()
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::Mutex;
    use std::thread;

    static PROXY_STATE_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn local_hosts_are_detected() {
        assert!(is_local_host("localhost"));
        assert!(is_local_host("LOCALHOST"));
        assert!(is_local_host("myhost.local"));
        assert!(is_local_host("127.0.0.1"));
        assert!(is_local_host("192.168.1.1"));
        assert!(is_local_host("10.0.0.1"));
        assert!(is_local_host("169.254.1.1"));
        assert!(is_local_host("::1"));
    }

    #[test]
    fn remote_hosts_are_not_local() {
        assert!(!is_local_host("example.com"));
        assert!(!is_local_host("8.8.8.8"));
        assert!(!is_local_host("2001:db8::1"));
        assert!(!is_local_host("node.monero.onion"));
    }

    #[test]
    fn bare_hostnames_are_not_local() {
        // Keep bare names on the proxied path.
        assert!(!is_local_host("singleword"));
        assert!(!is_local_host("intranet"));
        assert!(!is_local_host("wiki"));
    }

    #[test]
    fn enable_disable_toggle() {
        let _guard = PROXY_STATE_LOCK.lock().unwrap();

        disable();
        assert!(!is_enabled());
        assert!(proxy_config(Subsystem::Http).is_none());
        assert!(proxy_addr().is_none());

        enable();
        assert!(is_enabled());
        assert_eq!(
            proxy_config(Subsystem::Http).map(|proxy| proxy.url()),
            Some("socks5h://http:http@127.0.0.1:9050".to_string())
        );
        assert_eq!(proxy_addr(), Some(current_addr()));

        disable();
        assert!(!is_enabled());
    }

    #[test]
    fn pack_unpack_roundtrip() {
        let cases = [
            SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 9050),
            SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), 0),
            SocketAddrV4::new(Ipv4Addr::new(255, 255, 255, 255), 65535),
            SocketAddrV4::new(Ipv4Addr::new(10, 152, 152, 10), 9050),
        ];
        for addr in cases {
            assert_eq!(unpack(pack(addr)), addr);
        }
    }

    #[test]
    fn subsystem_credentials_are_isolated() {
        let _guard = PROXY_STATE_LOCK.lock().unwrap();

        enable();

        let http = proxy_config(Subsystem::Http).expect("http proxy config");
        let updater = proxy_config(Subsystem::Updater).expect("updater proxy config");

        assert_eq!(http.addr, current_addr());
        assert_eq!(updater.addr, current_addr());
        assert_ne!(http.url(), updater.url());

        disable();
    }

    #[test]
    fn probe_addr_str_rejects_malformed_input() {
        // Invalid IPv4 `ip:port` strings must fail locally.
        assert!(!probe_addr_str(""));
        assert!(!probe_addr_str("not-an-addr"));
        assert!(!probe_addr_str("127.0.0.1")); // missing port
        assert!(!probe_addr_str("localhost:9050")); // hostname, not IPv4
        assert!(!probe_addr_str("[::1]:9050")); // IPv6 not supported
    }

    #[test]
    fn probe_addr_rejects_non_socks5_tcp_service() {
        // Reject TCP services that do not speak SOCKS5.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let addr = match listener.local_addr().expect("local_addr") {
            std::net::SocketAddr::V4(v4) => v4,
            std::net::SocketAddr::V6(_) => panic!("expected IPv4 ephemeral bind"),
        };

        let server = thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                // Reply with non-SOCKS5 bytes.
                let _ = stream.write_all(&[0x00, 0x00]);
            }
        });

        assert!(!probe_addr(addr));
        let _ = server.join();
    }

    #[test]
    fn subsystem_proxy_url_for_does_not_require_enabled() {
        // This path must not depend on `enable()`.
        let _guard = PROXY_STATE_LOCK.lock().unwrap();
        disable();

        let addr = SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 9050);
        assert_eq!(
            Subsystem::Updater.proxy_url_for(addr),
            "socks5h://updater:updater@127.0.0.1:9050"
        );
        assert!(!is_enabled());
    }

    #[test]
    fn enable_with_addr_updates_current_addr() {
        let _guard = PROXY_STATE_LOCK.lock().unwrap();

        let whonix = SocketAddrV4::new(Ipv4Addr::new(10, 152, 152, 10), 9050);
        enable_with_addr(whonix);
        assert_eq!(current_addr(), whonix);
        assert_eq!(proxy_addr(), Some(whonix));

        disable();
        // `disable()` only clears ENABLED — the last address stays cached.
        assert_eq!(current_addr(), whonix);
        assert!(proxy_addr().is_none());
    }
}
