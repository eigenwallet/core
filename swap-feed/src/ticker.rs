use anyhow::{anyhow, Result};
use futures::stream::BoxStream;
use futures::TryStreamExt;
use std::convert::Infallible;
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::watch;

pub fn connect<Params, ConnectionNewRet, ConnectionNewError, WirePriceUpdate>(
    label: &'static str,
    params: Params,
    connection_new: fn(Params) -> ConnectionNewRet,
) -> Result<PriceUpdates<WirePriceUpdate>>
where
    Params: Clone + Send + Sync + 'static,
    ConnectionNewRet: Future<Output = Result<BoxStream<'static, Result<WirePriceUpdate, ConnectionNewError>>>>
        + Send
        + 'static,
    ConnectionNewError: std::error::Error + Send + Sync + 'static,
    WirePriceUpdate: Clone + Send + Sync + 'static,
{
    let (price_update, price_update_receiver) = watch::channel(Err(Error::NotYetAvailable));
    let price_update = Arc::new(price_update);

    tokio::spawn(async move {
        // The default backoff config is fine for us apart from one thing:
        // `max_elapsed_time`. If we don't get an error within this timeframe,
        // backoff won't actually retry the operation.
        let backoff = backoff::ExponentialBackoff {
            max_elapsed_time: None,
            ..backoff::ExponentialBackoff::default()
        };

        let result = backoff::future::retry_notify::<Infallible, _, _, _, _, _>(
            backoff,
            || {
                let price_update = price_update.clone();
                let params = params.clone();
                async move {
                    let mut stream = connection_new(params).await?;

                    while let Some(update) = stream
                        .try_next()
                        .await
                        .map_err(anyhow::Error::from)
                        .map_err(backoff::Error::transient)?
                    {
                        let send_result = price_update.send(Ok((Instant::now(), update)));

                        if send_result.is_err() {
                            return Err(backoff::Error::Permanent(anyhow!(
                                "receiver disconnected"
                            )));
                        }
                    }

                    Err(backoff::Error::transient(anyhow!("stream ended")))
                }
            },
            |error, next: Duration| {
                tracing::info!(
                    "{} websocket connection failed, retrying in {}ms. Error {:#}",
                    label,
                    next.as_millis(),
                    error
                );
            },
        )
        .await;

        let err = result.expect_err("Stream can't end successfully");
        tracing::warn!("Rate updates incurred an unrecoverable error: {:#}", err);

        // in case the retries fail permanently, let the subscribers know
        price_update.send(Err(Error::PermanentFailure(err.into())))
    });

    Ok(PriceUpdates {
        inner: price_update_receiver,
    })
}

#[derive(Clone, Debug)]
pub struct PriceUpdates<WirePriceUpdate: Clone + Send + 'static> {
    inner: watch::Receiver<PriceUpdate<WirePriceUpdate>>,
}

impl<WirePriceUpdate: Clone + Send + 'static> PriceUpdates<WirePriceUpdate> {
    pub async fn wait_for_next_update(&mut self) -> Result<PriceUpdate<WirePriceUpdate>> {
        self.inner.changed().await?;

        Ok(self.inner.borrow().clone())
    }

    pub fn latest_update(&mut self) -> PriceUpdate<WirePriceUpdate> {
        self.inner.borrow().clone()
    }
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum Error {
    #[error("Rate is not yet available")]
    NotYetAvailable,
    #[error("Permanently failed to retrieve rate from exchange")]
    PermanentFailure(Arc<anyhow::Error>),
}

type PriceUpdate<WirePriceUpdate> = Result<(Instant, WirePriceUpdate), Error>;
