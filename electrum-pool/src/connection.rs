use std::sync::Arc;
use std::time::Duration;

use electrum_streaming_client::client::AsyncRequestError;
use electrum_streaming_client::{AsyncClient, RequestExt};
use once_cell::sync::OnceCell;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};

use crate::Error;

/// A single live connection to one Electrum server.
///
/// Wraps an [`AsyncClient`] over a TCP or TLS stream together with the spawned worker task that
/// drives the socket. Requests are issued with [`Connection::request`] and time out after the
/// configured duration; a timed-out or transport-failed request yields an [`Error::Connection`]
/// so the balancer can fail over and reconnect.
pub struct Connection {
    url: String,
    client: AsyncClient,
    worker: tokio::task::JoinHandle<std::io::Result<()>>,
    request_timeout: Duration,
}

impl Connection {
    /// Connect to the given `url` (`tcp://` or `ssl://`), spawning the client worker on the current
    /// tokio runtime. The whole connect (incl. TLS handshake) is bounded by `request_timeout`.
    pub async fn connect(url: &str, request_timeout: Duration) -> Result<Self, Error> {
        let target = ConnectionTarget::parse(url)?;

        let tcp = tokio::time::timeout(
            request_timeout,
            TcpStream::connect((target.host.as_str(), target.port)),
        )
        .await
        .map_err(|_| Error::connection(format!("Timed out connecting to {url}")))?
        .map_err(|e| Error::connection(dns_hint(url, e)))?;

        let _ = tcp.set_nodelay(true);

        let (client, worker) = if target.use_tls {
            let connector = TlsConnector::from(tls_config());
            let server_name = ServerName::try_from(target.host.clone())
                .map_err(|e| Error::connection(format!("Invalid TLS server name for {url}: {e}")))?;
            let tls = tokio::time::timeout(request_timeout, connector.connect(server_name, tcp))
                .await
                .map_err(|_| Error::connection(format!("Timed out during TLS handshake to {url}")))?
                .map_err(|e| Error::connection(format!("TLS handshake failed for {url}: {e}")))?;
            let (reader, writer) = tokio::io::split(tls);
            spawn_client(reader, writer)
        } else {
            let (reader, writer) = tcp.into_split();
            spawn_client(reader, writer)
        };

        Ok(Self {
            url: url.to_string(),
            client,
            worker,
            request_timeout,
        })
    }

    /// Issue a single tracked request and await the typed response, bounded by the request timeout.
    pub async fn request<Req>(&self, req: Req) -> Result<Req::Response, Error>
    where
        Req: RequestExt + Send + Sync + 'static,
        Req::Response: Send,
    {
        match tokio::time::timeout(self.request_timeout, self.client.send_request(req)).await {
            Ok(Ok(resp)) => Ok(resp),
            Ok(Err(AsyncRequestError::Response(resp_err))) => Err(Error::response(&resp_err)),
            Ok(Err(AsyncRequestError::Canceled)) => {
                Err(Error::connection("Request canceled (connection closed)"))
            }
            Ok(Err(AsyncRequestError::Dispatch(e))) => {
                Err(Error::connection(format!("Failed to dispatch request: {e}")))
            }
            Err(_elapsed) => Err(Error::connection("Request timed out")),
        }
    }

    /// The URL this connection was created from.
    pub fn url(&self) -> &str {
        &self.url
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.worker.abort();
    }
}

fn spawn_client<R, W>(reader: R, writer: W) -> (AsyncClient, tokio::task::JoinHandle<std::io::Result<()>>)
where
    R: tokio::io::AsyncRead + Send + Unpin + 'static,
    W: tokio::io::AsyncWrite + Send + Unpin + 'static,
{
    let (client, mut events, worker) = AsyncClient::new_tokio(reader, writer);

    // We only use request/response (callback-tracked) requests, so no notifications are produced.
    // Still drain the event stream so a stray notification can never wedge the worker loop.
    tokio::spawn(async move {
        use futures::StreamExt;
        while events.next().await.is_some() {}
    });

    (client, tokio::spawn(worker))
}

struct ConnectionTarget {
    host: String,
    port: u16,
    use_tls: bool,
}

impl ConnectionTarget {
    fn parse(url: &str) -> Result<Self, Error> {
        let (scheme, rest) = url
            .split_once("://")
            .ok_or_else(|| Error::connection(format!("Missing scheme in Electrum URL: {url}")))?;

        let use_tls = match scheme {
            "tcp" => false,
            "ssl" | "tls" => true,
            other => {
                return Err(Error::connection(format!(
                    "Unsupported Electrum URL scheme `{other}` in {url}"
                )));
            }
        };

        // Strip optional `user:pass@` credentials (only host:port is significant for us).
        let host_port = rest.rsplit_once('@').map(|(_, hp)| hp).unwrap_or(rest);

        let (host, port) = host_port
            .rsplit_once(':')
            .ok_or_else(|| Error::connection(format!("Missing port in Electrum URL: {url}")))?;

        let port: u16 = port
            .parse()
            .map_err(|_| Error::connection(format!("Invalid port in Electrum URL: {url}")))?;

        if host.is_empty() {
            return Err(Error::connection(format!("Empty host in Electrum URL: {url}")));
        }

        Ok(Self {
            host: host.to_string(),
            port,
            use_tls,
        })
    }
}

/// Wrap a connect IO error with the legacy DNS-resolution hint for the failure kinds that most
/// commonly indicate an unresolvable/unreachable host.
fn dns_hint(url: &str, e: std::io::Error) -> String {
    use std::io::ErrorKind::*;
    match e.kind() {
        NotFound | TimedOut | ConnectionRefused | ConnectionAborted | Other => {
            format!("{url}: {e} (Most likely DNS resolution error)")
        }
        _ => format!("{url}: {e}"),
    }
}

fn tls_config() -> Arc<ClientConfig> {
    static CONFIG: OnceCell<Arc<ClientConfig>> = OnceCell::new();
    CONFIG
        .get_or_init(|| {
            let mut roots = RootCertStore::empty();
            let loaded = rustls_native_certs::load_native_certs();
            for cert in loaded.certs {
                let _ = roots.add(cert);
            }

            let provider = Arc::new(tokio_rustls::rustls::crypto::ring::default_provider());
            let config = ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .expect("ring provider supports the safe default protocol versions")
                .with_root_certificates(roots)
                .with_no_client_auth();

            Arc::new(config)
        })
        .clone()
}
