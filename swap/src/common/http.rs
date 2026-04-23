use anyhow::{Context, Result};
use reqwest::Url;
use std::time::Duration;
use tor_socks5::Subsystem;

/// Build a `reqwest::Client` for `url`.
///
/// Local addresses bypass the proxy.
pub fn build_http_client(
    url: &Url,
    timeout: Duration,
    subsystem: Subsystem,
    user_agent: Option<&str>,
) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder().timeout(timeout);
    if let Some(user_agent) = user_agent {
        builder = builder.user_agent(user_agent);
    }
    configure_http_client(builder, url, subsystem)?
        .build()
        .context("Failed to build HTTP client")
}

pub fn configure_http_client(
    mut builder: reqwest::ClientBuilder,
    url: &Url,
    subsystem: Subsystem,
) -> Result<reqwest::ClientBuilder> {
    if should_bypass_proxy(url) {
        tracing::debug!(%url, "Bypassing system Tor SOCKS5 for local or LAN address");
        return Ok(builder);
    }

    if let Some(proxy) = tor_socks5::proxy_config(subsystem) {
        tracing::debug!(%url, proxy = %proxy.url(), "Using system Tor SOCKS5 for HTTP request");

        builder = builder.proxy(
            proxy
                .reqwest_proxy()
                .context("Failed to configure system Tor SOCKS5 proxy")?,
        );
    }

    Ok(builder)
}

fn should_bypass_proxy(url: &Url) -> bool {
    match url.host_str() {
        None => false,
        Some(host) => tor_socks5::is_local_host(host),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, SocketAddrV4};
    use std::sync::Mutex;

    /// Tests that toggle `tor_socks5` must not run in parallel.
    static PROXY_STATE_LOCK: Mutex<()> = Mutex::new(());

    fn url(s: &str) -> Url {
        Url::parse(s).expect("valid test URL")
    }

    #[test]
    fn should_bypass_proxy_for_local_urls() {
        assert!(should_bypass_proxy(&url("http://localhost:1234")));
        assert!(should_bypass_proxy(&url("http://127.0.0.1:9050")));
        assert!(should_bypass_proxy(&url("http://10.0.0.1")));
        assert!(should_bypass_proxy(&url("http://192.168.1.1:80")));
        assert!(should_bypass_proxy(&url("https://myhost.local")));
    }

    #[test]
    fn should_not_bypass_proxy_for_remote_urls() {
        assert!(!should_bypass_proxy(&url("https://example.com")));
        assert!(!should_bypass_proxy(&url("https://api.coingecko.com/api")));
        // Bare single-label hosts stay on the proxied path.
        assert!(!should_bypass_proxy(&url("http://intranet")));
    }

    #[test]
    fn build_http_client_is_infallible_for_both_modes() {
        let _guard = PROXY_STATE_LOCK.lock().unwrap();

        // Building without a proxy must succeed.
        tor_socks5::disable();
        assert!(
            build_http_client(
                &url("https://example.com"),
                Duration::from_secs(5),
                Subsystem::Http,
                None,
            )
            .is_ok()
        );

        // Both proxied and bypassed requests must build.
        tor_socks5::enable_with_addr(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 9050));
        assert!(
            build_http_client(
                &url("https://example.com"),
                Duration::from_secs(5),
                Subsystem::MoneroRpc,
                Some("eigenwallet-test/0.0"),
            )
            .is_ok()
        );
        assert!(
            build_http_client(
                &url("http://127.0.0.1:18081"),
                Duration::from_secs(5),
                Subsystem::MoneroRpc,
                None,
            )
            .is_ok()
        );

        tor_socks5::disable();
    }
}
