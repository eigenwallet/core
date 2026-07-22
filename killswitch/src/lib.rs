use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use tokio::process::{Child, Command};
use tracing_subscriber as _;
use url::Url;

pub const DEFAULT_ENDPOINT: &str = "https://killswitch.eigenwallet.org/api/asb";
pub const DEFAULT_STATE_FILE: &str = "/run/killswitch/state.json";
pub const HEALTHY_POLL_INTERVAL: Duration = Duration::from_secs(60);
pub const MAX_RETRY_INTERVAL: Duration = Duration::from_secs(10);
pub const MAX_STATE_AGE: Duration = Duration::from_secs(75);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const SUPERVISOR_POLL_INTERVAL: Duration = Duration::from_secs(1);
const CHILD_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_REASON_BYTES: u64 = 4096;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct State {
    pub active: bool,
    pub reason: Option<String>,
    pub checked_at_unix_seconds: u64,
}

#[derive(Debug, Deserialize)]
struct ActiveResponse {
    reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Observation {
    Inactive,
    Active { reason: Option<String> },
}

pub async fn monitor(endpoint: Url, state_file: PathBuf) -> Result<()> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(REQUEST_TIMEOUT)
        .build()
        .context("Failed to build killswitch HTTP client")?;
    let mut previous = None;
    let mut retry = Retry::default();

    loop {
        let observation = match check_endpoint(&client, &endpoint).await {
            Ok(observation) => observation,
            Err(error) => {
                tracing::warn!(%error, "Killswitch request failed; failing closed");
                Observation::Active {
                    reason: Some(error.to_string()),
                }
            }
        };

        write_state(&state_file, &observation).await?;
        log_transition(previous.as_ref(), &observation);

        let sleep_for = if observation == Observation::Inactive {
            retry.reset();
            HEALTHY_POLL_INTERVAL
        } else {
            retry.next()
        };
        previous = Some(observation);
        tokio::time::sleep(sleep_for).await;
    }
}

pub async fn health(state_file: &Path) -> Result<()> {
    ensure_inactive(state_file).await
}

pub async fn supervise(state_file: PathBuf, command: Vec<String>) -> Result<()> {
    let Some((program, arguments)) = command.split_first() else {
        bail!("No ASB command provided to killswitch supervisor");
    };
    let mut blocked_reason = None;

    loop {
        match ensure_inactive(&state_file).await {
            Ok(()) => {
                if blocked_reason.take().is_some() {
                    tracing::info!("Killswitch cleared; starting ASB");
                }
            }
            Err(error) => {
                let reason = error.to_string();
                if blocked_reason.as_deref() != Some(reason.as_str()) {
                    tracing::warn!(%reason, "ASB start blocked by killswitch");
                    blocked_reason = Some(reason);
                }
                tokio::time::sleep(SUPERVISOR_POLL_INTERVAL).await;
                continue;
            }
        }

        tracing::info!(program, "Starting supervised ASB process");
        let mut child = Command::new(program)
            .args(arguments)
            .spawn()
            .with_context(|| format!("Failed to start supervised process `{program}`"))?;

        supervise_child(&state_file, &mut child).await?;
    }
}

async fn check_endpoint(client: &reqwest::Client, endpoint: &Url) -> Result<Observation> {
    let response = client
        .get(endpoint.clone())
        .send()
        .await
        .with_context(|| format!("Failed to fetch {endpoint}"))?;

    if response.status() == reqwest::StatusCode::OK {
        return Ok(Observation::Inactive);
    }

    let status = response.status();
    let reason = read_reason(response).await;
    let reason = reason.or_else(|| Some(format!("Killswitch endpoint returned HTTP {status}")));

    Ok(Observation::Active { reason })
}

async fn read_reason(mut response: reqwest::Response) -> Option<String> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_REASON_BYTES)
    {
        return None;
    }

    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if body.len().saturating_add(chunk.len()) > MAX_REASON_BYTES as usize {
            return None;
        }
        body.extend_from_slice(&chunk);
    }

    serde_json::from_slice::<ActiveResponse>(&body)
        .ok()?
        .reason
        .filter(|reason| !reason.trim().is_empty())
}

async fn write_state(path: &Path, observation: &Observation) -> Result<()> {
    let parent = path
        .parent()
        .context("Killswitch state file must have a parent directory")?;
    tokio::fs::create_dir_all(parent)
        .await
        .with_context(|| format!("Failed to create {}", parent.display()))?;

    let state = match observation {
        Observation::Inactive => State {
            active: false,
            reason: None,
            checked_at_unix_seconds: unix_timestamp()?,
        },
        Observation::Active { reason } => State {
            active: true,
            reason: reason.clone(),
            checked_at_unix_seconds: unix_timestamp()?,
        },
    };
    let contents = serde_json::to_vec(&state).context("Failed to serialize killswitch state")?;
    let temporary = path.with_extension("tmp");

    tokio::fs::write(&temporary, contents)
        .await
        .with_context(|| format!("Failed to write {}", temporary.display()))?;
    tokio::fs::rename(&temporary, path)
        .await
        .with_context(|| format!("Failed to replace {}", path.display()))?;
    Ok(())
}

async fn ensure_inactive(path: &Path) -> Result<()> {
    let contents = tokio::fs::read(path)
        .await
        .with_context(|| format!("Failed to read killswitch state from {}", path.display()))?;
    let state: State = serde_json::from_slice(&contents)
        .with_context(|| format!("Failed to parse killswitch state from {}", path.display()))?;

    if state.active {
        bail!(
            "Killswitch is active{}",
            state
                .reason
                .as_deref()
                .map(|reason| format!(": {reason}"))
                .unwrap_or_default()
        );
    }

    let age = unix_timestamp()?
        .checked_sub(state.checked_at_unix_seconds)
        .ok_or_else(|| anyhow!("Killswitch state timestamp is in the future"))?;
    if age > MAX_STATE_AGE.as_secs() {
        bail!("Killswitch state is stale ({age} seconds old)");
    }

    Ok(())
}

async fn supervise_child(state_file: &Path, child: &mut Child) -> Result<()> {
    loop {
        tokio::select! {
            status = child.wait() => {
                let status = status.context("Failed to wait for supervised ASB process")?;
                bail!("Supervised ASB process exited unexpectedly with {status}");
            }
            () = tokio::time::sleep(SUPERVISOR_POLL_INTERVAL) => {
                if let Err(error) = ensure_inactive(state_file).await {
                    tracing::error!(%error, "Stopping ASB because the killswitch is active");
                    terminate_child(child).await?;
                    return Ok(());
                }
            }
        }
    }
}

async fn terminate_child(child: &mut Child) -> Result<()> {
    let pid = child
        .id()
        .context("Supervised ASB process has no process ID")?;
    let result = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
    if result != 0 {
        return Err(std::io::Error::last_os_error()).context("Failed to send SIGTERM to ASB");
    }

    match tokio::time::timeout(CHILD_SHUTDOWN_TIMEOUT, child.wait()).await {
        Ok(status) => {
            status.context("Failed to wait for ASB shutdown")?;
        }
        Err(_) => {
            tracing::warn!(pid, "ASB did not stop after SIGTERM; sending SIGKILL");
            child
                .kill()
                .await
                .context("Failed to send SIGKILL to ASB")?;
        }
    }
    Ok(())
}

fn log_transition(previous: Option<&Observation>, current: &Observation) {
    if previous == Some(current) {
        return;
    }

    match current {
        Observation::Inactive => tracing::info!("Killswitch is inactive"),
        Observation::Active { reason } => {
            tracing::error!(reason, "Killswitch is active");
        }
    }
}

fn unix_timestamp() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("System clock is before the Unix epoch")?
        .as_secs())
}

#[derive(Default)]
struct Retry {
    attempt: u32,
}

impl Retry {
    fn next(&mut self) -> Duration {
        let seconds = 1_u64
            .checked_shl(self.attempt.min(4))
            .unwrap_or(MAX_RETRY_INTERVAL.as_secs())
            .min(MAX_RETRY_INTERVAL.as_secs());
        self.attempt = self.attempt.saturating_add(1);
        Duration::from_secs(seconds)
    }

    fn reset(&mut self) {
        self.attempt = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;

    #[tokio::test]
    async fn health_accepts_fresh_inactive_state() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.json");
        write_state(&path, &Observation::Inactive).await.unwrap();

        health(&path).await.unwrap();
    }

    #[tokio::test]
    async fn health_rejects_active_state() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.json");
        write_state(
            &path,
            &Observation::Active {
                reason: Some("maintenance".to_string()),
            },
        )
        .await
        .unwrap();

        let error = health(&path).await.unwrap_err();
        assert!(error.to_string().contains("maintenance"));
    }

    #[tokio::test]
    async fn health_rejects_stale_state() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.json");
        let state = State {
            active: false,
            reason: None,
            checked_at_unix_seconds: unix_timestamp().unwrap() - MAX_STATE_AGE.as_secs() - 1,
        };
        tokio::fs::write(&path, serde_json::to_vec(&state).unwrap())
            .await
            .unwrap();

        let error = health(&path).await.unwrap_err();
        assert!(error.to_string().contains("stale"));
    }

    #[tokio::test]
    async fn health_rejects_missing_state() {
        let directory = tempfile::tempdir().unwrap();

        let error = health(&directory.path().join("missing.json"))
            .await
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("Failed to read killswitch state")
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn supervisor_stops_running_child_when_state_becomes_active() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.json");
        write_state(&path, &Observation::Inactive).await.unwrap();
        let mut child = Command::new("sh")
            .args(["-c", "exec sleep 30"])
            .spawn()
            .unwrap();
        write_state(
            &path,
            &Observation::Active {
                reason: Some("maintenance".to_string()),
            },
        )
        .await
        .unwrap();

        supervise_child(&path, &mut child).await.unwrap();

        assert!(child.try_wait().unwrap().is_some());
    }

    #[test]
    fn retry_is_capped_and_resettable() {
        let mut retry = Retry::default();
        let intervals: Vec<_> = (0..7).map(|_| retry.next().as_secs()).collect();
        assert_eq!(intervals, vec![1, 2, 4, 8, 10, 10, 10]);

        retry.reset();
        assert_eq!(retry.next(), Duration::from_secs(1));
    }

    #[tokio::test]
    async fn http_200_is_inactive() {
        let endpoint = serve_response("200 OK", "{}");
        let client = reqwest::Client::new();

        let observation = check_endpoint(&client, &endpoint).await.unwrap();

        assert_eq!(observation, Observation::Inactive);
    }

    #[tokio::test]
    async fn non_200_is_active_and_preserves_reason() {
        let endpoint = serve_response("503 Service Unavailable", r#"{"reason":"maintenance"}"#);
        let client = reqwest::Client::new();

        let observation = check_endpoint(&client, &endpoint).await.unwrap();

        assert_eq!(
            observation,
            Observation::Active {
                reason: Some("maintenance".to_string())
            }
        );
    }

    #[tokio::test]
    async fn active_reason_does_not_require_content_length() {
        let endpoint = serve_raw_response(
            "503 Service Unavailable",
            "Transfer-Encoding: chunked\r\n".to_string(),
            "18\r\n{\"reason\":\"maintenance\"}\r\n0\r\n\r\n".to_string(),
        );
        let client = reqwest::Client::new();

        let observation = check_endpoint(&client, &endpoint).await.unwrap();

        assert_eq!(
            observation,
            Observation::Active {
                reason: Some("maintenance".to_string())
            }
        );
    }

    fn serve_response(status: &'static str, body: &'static str) -> Url {
        serve_raw_response(
            status,
            format!("Content-Length: {}\r\n", body.len()),
            body.to_string(),
        )
    }

    fn serve_raw_response(status: &'static str, headers: String, body: String) -> Url {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).unwrap();
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{headers}Connection: close\r\n\r\n{body}",
            )
            .unwrap();
        });
        Url::parse(&format!("http://{address}/api/asb")).unwrap()
    }
}
