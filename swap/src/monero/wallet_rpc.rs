use ::monero::Network;
use anyhow::{Context, Error, Result};
use serde::Deserialize;
use std::fmt;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone)]
pub struct MoneroDaemon {
    url: String,
    network: Network,
}

impl MoneroDaemon {
    pub fn new(url: impl Into<String>, network: Network) -> MoneroDaemon {
        MoneroDaemon {
            url: url.into(),
            network,
        }
    }

    pub fn from_str(url: impl Into<String>, network: Network) -> Result<MoneroDaemon, Error> {
        Ok(MoneroDaemon {
            url: url.into(),
            network,
        })
    }

    /// Checks if the Monero daemon is available by sending a request to its `get_info` endpoint.
    pub async fn is_available(&self, client: &reqwest::Client) -> Result<bool, Error> {
        let url = if self.url.ends_with("/") {
            format!("{}get_info", self.url)
        } else {
            format!("{}/get_info", self.url)
        };

        let res = client
            .get(&url)
            .send()
            .await
            .context("Failed to send request to get_info endpoint")?;

        let json: MoneroDaemonGetInfoResponse = res
            .json()
            .await
            .context("Failed to deserialize daemon get_info response")?;

        let is_status_ok = json.status == "OK";
        let is_synchronized = json.synchronized;
        let is_correct_network = match self.network {
            Network::Mainnet => json.mainnet,
            Network::Stagenet => json.stagenet,
            Network::Testnet => json.testnet,
        };

        Ok(is_status_ok && is_synchronized && is_correct_network)
    }
}

impl Display for MoneroDaemon {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.url)
    }
}

#[derive(Deserialize)]
struct MoneroDaemonGetInfoResponse {
    status: String,
    synchronized: bool,
    mainnet: bool,
    stagenet: bool,
    testnet: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_is_daemon_available_success() {
        let mut server = mockito::Server::new_async().await;

        let _ = server
            .mock("GET", "/get_info")
            .with_status(200)
            .with_body(
                r#"
                {
                    "status": "OK",
                    "synchronized": true,
                    "mainnet": true,
                    "stagenet": false,
                    "testnet": false
                }
                "#,
            )
            .create();

        let url = server.url();

        let client = reqwest::Client::new();
        let result = MoneroDaemon::new(url, Network::Mainnet)
            .is_available(&client)
            .await;

        assert!(result.unwrap());
    }

    #[tokio::test]
    async fn test_is_daemon_available_wrong_network_failure() {
        let mut server = mockito::Server::new_async().await;

        let _ = server
            .mock("GET", "/get_info")
            .with_status(200)
            .with_body(
                r#"
                {
                    "status": "OK",
                    "synchronized": true,
                    "mainnet": true,
                    "stagenet": false,
                    "testnet": false
                }
                "#,
            )
            .create();

        let url = server.url();

        let client = reqwest::Client::new();
        let result = MoneroDaemon::new(url, Network::Stagenet)
            .is_available(&client)
            .await;

        assert!(!result.unwrap());
    }

    #[tokio::test]
    async fn test_is_daemon_available_not_synced_failure() {
        let mut server = mockito::Server::new_async().await;

        let _ = server
            .mock("GET", "/get_info")
            .with_status(200)
            .with_body(
                r#"
                {
                    "status": "OK",
                    "synchronized": false,
                    "mainnet": true,
                    "stagenet": false,
                    "testnet": false
                }
                "#,
            )
            .create();

        let url = server.url();

        let client = reqwest::Client::new();
        let result = MoneroDaemon::new(url, Network::Mainnet)
            .is_available(&client)
            .await;

        assert!(!result.unwrap());
    }

    #[tokio::test]
    async fn test_is_daemon_available_network_error_failure() {
        let client = reqwest::Client::new();
        let result = MoneroDaemon::new("http://does.not.exist.com:18081", Network::Mainnet)
            .is_available(&client)
            .await;

        assert!(result.is_err());
    }
}
