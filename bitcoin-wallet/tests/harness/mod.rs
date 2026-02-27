pub mod bitcoind;
pub mod electrs;

use anyhow::{Context, Result};
use bitcoin_harness::{BitcoindRpcApi, Client as BitcoindClient};
use testcontainers::clients::Cli;
use testcontainers::{Container, RunnableImage};
use url::Url;

pub const BITCOIN_TEST_WALLET_NAME: &str = "bitcoin-wallet-it";

#[allow(dead_code)]
pub struct TestEnv<'a> {
    pub electrum_url: String,
    pub bitcoind_url: Url,
    pub bitcoind: BitcoindClient,
    pub electrs_port: u16,
    _bitcoind_container: Container<'a, bitcoind::Bitcoind>,
    _electrs_container: Container<'a, electrs::Electrs>,
}

pub async fn setup<'a>(cli: &'a Cli) -> Result<TestEnv<'a>> {
    ensure_docker_available()?;

    let prefix = random_prefix();
    let network = format!("{}-btc", prefix);
    let bitcoind_name = format!("{}-bitcoind", prefix);

    let (bitcoind_container, bitcoind_url) = init_bitcoind_container(cli, prefix.clone(), bitcoind_name.clone(), network.clone())
        .await
        .context("init bitcoind container")?;

    let electrs_container = init_electrs_container(cli, prefix, bitcoind_name, network, bitcoind::RPC_PORT)
        .await
        .context("init electrs container")?;

    let electrs_port = electrs_container.get_host_port_ipv4(electrs::RPC_PORT);
    // Use a plain TCP electrum URL; we explicitly wait for electrs readiness below.
    let electrum_url = format!("tcp://127.0.0.1:{}", electrs_port);

    let bitcoind = BitcoindClient::new(bitcoind_url.clone());

    // Ensure bitcoind has a wallet with mature coins we can spend from.
    init_bitcoind_wallet(&bitcoind).await?;

    // Electrs can print its "ready" line before it's actually able to serve requests.
    // Wait until it answers at least one basic RPC call.
    wait_for_electrs(&electrum_url).await?;

    Ok(TestEnv {
        electrum_url,
        bitcoind_url,
        bitcoind,
        electrs_port,
        _bitcoind_container: bitcoind_container,
        _electrs_container: electrs_container,
    })
}

async fn wait_for_electrs(url: &str) -> Result<()> {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::{Duration, Instant};

    let deadline = Instant::now() + Duration::from_secs(30);
    let (host, port) = parse_tcp_electrum_host_port(url)?;

    loop {
        let host = host.clone();
        let res = tokio::task::spawn_blocking(move || {
            let addr = (host.as_str(), port);
            let mut stream = TcpStream::connect(addr)
                .with_context(|| format!("failed to connect to electrs at {host}:{port}"))?;
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .context("failed to set read timeout")?;
            stream
                .set_write_timeout(Some(Duration::from_secs(2)))
                .context("failed to set write timeout")?;

            // Minimal Electrum JSON-RPC request.
            // If electrs isn't ready, it may close immediately (EOF) or not respond.
            let req = b"{\"id\":0,\"method\":\"server.version\",\"params\":[\"bitcoin-wallet-it\",\"1.4\"]}\n";
            stream.write_all(req).context("failed to write electrum request")?;
            stream.flush().ok();

            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).context("failed to read electrum response")?;
            if n == 0 {
                anyhow::bail!("EOF")
            }

            let s = String::from_utf8_lossy(&buf[..n]);
            if !s.contains("\"result\"") {
                anyhow::bail!("unexpected electrum response: {s}")
            }

            Ok::<(), anyhow::Error>(())
        })
        .await
        .context("failed to join electrs readiness task")?;

        match res {
            Ok(()) => return Ok(()),
            Err(e) => {
                if Instant::now() >= deadline {
                    anyhow::bail!("electrs did not become ready in time: {e}")
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    }
}

fn parse_tcp_electrum_host_port(url: &str) -> Result<(String, u16)> {
    // Expected forms in this repo:
    // - tcp://@127.0.0.1:50001
    // - tcp://@localhost:50001
    // - tcp://user:pass@host:port
    let rest = url
        .strip_prefix("tcp://")
        .ok_or_else(|| anyhow::anyhow!("unsupported electrum url scheme: {url}"))?;

    let host_port = rest
        .rsplit('@')
        .next()
        .unwrap_or(rest)
        .trim();

    let mut parts = host_port.split(':');
    let host = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("missing host in electrum url: {url}"))?
        .to_string();
    let port_str = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("missing port in electrum url: {url}"))?;
    let port: u16 = port_str
        .parse()
        .with_context(|| format!("invalid port in electrum url: {url}"))?;

    Ok((host, port))
}

fn ensure_docker_available() -> Result<()> {
    let output = std::process::Command::new("docker")
        .arg("info")
        .output()
        .context("failed to execute `docker info` (is Docker installed?)")?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow::bail!(
        "Docker daemon is not reachable. Start Docker Desktop (or the Docker daemon) and re-run the tests.\n\n`docker info` error:\n{stderr}"
    )
}

pub async fn fund_and_mine(
    bitcoind: &BitcoindClient,
    recipient: bitcoin::Address,
    amount: bitcoin::Amount,
) -> Result<()> {
    bitcoind
        .send_to_address(BITCOIN_TEST_WALLET_NAME, recipient, amount)
        .await
        .context("send_to_address")?;

    let miner_addr = bitcoind
        .with_wallet(BITCOIN_TEST_WALLET_NAME)?
        .getnewaddress(None, None)
        .await
        .context("getnewaddress")?;

    let miner_addr = miner_addr.require_network(bitcoind.network().await?)?;

    bitcoind
        .generatetoaddress(1, miner_addr)
        .await
        .context("generatetoaddress")?;

    // We don't get the txid directly from send_to_address in this harness; for our
    // wallet tests it's enough that a tx is confirmed and shows up after sync.
    // Callers can query the wallet for the txid if needed.
    Ok(())
}

async fn init_bitcoind_wallet(bitcoind: &BitcoindClient) -> Result<()> {
    // Idempotent-ish: if wallet exists, createwallet will error. We treat that as OK.
    let _ = bitcoind
        .createwallet(BITCOIN_TEST_WALLET_NAME, None, None, None, None)
        .await;

    let reward_address = bitcoind
        .with_wallet(BITCOIN_TEST_WALLET_NAME)?
        .getnewaddress(None, None)
        .await
        .context("getnewaddress")?;

    let reward_address = reward_address.require_network(bitcoind.network().await?)?;

    // Mine enough blocks so coinbase is spendable.
    bitcoind
        .generatetoaddress(101, reward_address)
        .await
        .context("initial mining")?;

    Ok(())
}

async fn init_bitcoind_container<'a>(
    cli: &'a Cli,
    volume: String,
    name: String,
    network: String,
) -> Result<(Container<'a, bitcoind::Bitcoind>, Url)> {
    let image = bitcoind::Bitcoind::default().with_volume(volume);
    let image = RunnableImage::from(image)
        .with_container_name(name)
        .with_network(network);

    let docker = cli.run(image);
    let host_rpc_port = docker.get_host_port_ipv4(bitcoind::RPC_PORT);

    let bitcoind_url = {
        let input = format!(
            "http://{}:{}@127.0.0.1:{}",
            bitcoind::RPC_USER,
            bitcoind::RPC_PASSWORD,
            host_rpc_port
        );
        Url::parse(&input).expect("valid bitcoind rpc url")
    };

    Ok((docker, bitcoind_url))
}

async fn init_electrs_container<'a>(
    cli: &'a Cli,
    volume: String,
    bitcoind_container_name: String,
    network: String,
    bitcoind_rpc_port_in_network: u16,
) -> Result<Container<'a, electrs::Electrs>> {
    let bitcoind_rpc_addr = format!("{}:{}", bitcoind_container_name, bitcoind_rpc_port_in_network);
    let image = electrs::Electrs::default()
        .with_volume(volume)
        .with_daemon_rpc_addr(bitcoind_rpc_addr)
        .with_tag("latest");

    let image = RunnableImage::from(image.self_and_args())
        .with_network(network.clone())
        .with_container_name(format!("{}_electrs", network));

    Ok(cli.run(image))
}

fn random_prefix() -> String {
    use rand::distributions::Alphanumeric;
    use rand::{thread_rng, Rng};
    use std::iter;

    const LEN: usize = 8;

    let mut rng = thread_rng();
    iter::repeat(())
        .map(|()| rng.sample(Alphanumeric))
        .map(char::from)
        .take(LEN)
        .collect()
}
