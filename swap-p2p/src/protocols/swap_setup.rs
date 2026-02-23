use anyhow::{Context, Result};
use asynchronous_codec::{Bytes, Framed};
use futures::{SinkExt, StreamExt};

use libp2p::swarm::Stream;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub mod alice;
pub mod bob;
mod vendor_from_fn;

pub const BUF_SIZE: usize = 1024 * 1024;

pub mod protocol {
    use futures::future;
    use libp2p::core::Endpoint;
    use libp2p::swarm::Stream;
    use void::Void;

    use super::vendor_from_fn::{FromFnUpgrade, from_fn};

    pub fn new() -> SwapSetup {
        from_fn(
            "/comit/xmr/btc/swap_setup/1.0.0",
            Box::new(|socket, _| future::ready(Ok(socket))),
        )
    }

    pub type SwapSetup = FromFnUpgrade<
        &'static str,
        Box<dyn Fn(Stream, Endpoint) -> future::Ready<Result<Stream, Void>> + Send + 'static>,
    >;
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockchainNetwork {
    #[serde(with = "swap_serde::bitcoin::network")]
    pub bitcoin: bitcoin::Network,
    #[serde(with = "swap_serde::monero::network")]
    pub monero: monero_address::Network,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SpotPriceRequest {
    pub btc: bitcoin::Amount,
    pub blockchain_network: BlockchainNetwork,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum SpotPriceResponse {
    Xmr(swap_core::monero::Amount),
    Error(SpotPriceError),
}

#[derive(Clone, Debug, Serialize, Deserialize, thiserror::Error)]
pub enum SwapSetupError {
    #[error("Anti-spam deposit ({amount}) doesn't cover fees (minimum: {minimum_to_cover_fees})")]
    AntiSpamDepositTooSmall {
        amount: bitcoin::Amount,
        minimum_to_cover_fees: bitcoin::Amount,
    },
    #[error("Anti-spam deposit ratio ({ratio}) exceeds maximum accepted ({max_accepted_ratio})")]
    AntiSpamDepositRatioTooHigh {
        ratio: rust_decimal::Decimal,
        max_accepted_ratio: rust_decimal::Decimal,
    },
}

impl From<swap_machine::common::SanityCheckError> for SwapSetupError {
    fn from(err: swap_machine::common::SanityCheckError) -> Self {
        match err {
            swap_machine::common::SanityCheckError::AntiSpamDepositTooSmall {
                amount,
                minimum_to_cover_fees,
            } => SwapSetupError::AntiSpamDepositTooSmall {
                amount,
                minimum_to_cover_fees,
            },
            swap_machine::common::SanityCheckError::AntiSpamDepositRatioTooHigh {
                ratio,
                max_accepted_ratio,
            } => SwapSetupError::AntiSpamDepositRatioTooHigh {
                ratio,
                max_accepted_ratio,
            },
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SpotPriceError {
    NoSwapsAccepted,
    AmountBelowMinimum {
        min: bitcoin::Amount,
        buy: bitcoin::Amount,
    },
    AmountAboveMaximum {
        max: bitcoin::Amount,
        buy: bitcoin::Amount,
    },
    BalanceTooLow {
        buy: bitcoin::Amount,
    },
    BlockchainNetworkMismatch {
        cli: BlockchainNetwork,
        asb: BlockchainNetwork,
    },
    /// To be used for errors that cannot be explained on the CLI side (e.g.
    /// rate update problems on the seller side)
    Other,
}

fn codec() -> unsigned_varint::codec::UviBytes<Bytes> {
    let mut codec = unsigned_varint::codec::UviBytes::<Bytes>::default();
    codec.set_max_len(BUF_SIZE);
    codec
}

pub async fn read_cbor_message<T>(stream: &mut Stream) -> Result<Result<T, SwapSetupError>>
where
    T: DeserializeOwned,
{
    let mut frame = Framed::new(stream, codec());

    let bytes = frame
        .next()
        .await
        .context("Failed to read length-prefixed message from stream")??;

    let mut de = serde_cbor::Deserializer::from_slice(&bytes);
    let message = Result::<T, SwapSetupError>::deserialize(&mut de)
        .context("Failed to deserialize bytes into message using CBOR")?;

    Ok(message)
}

pub async fn write_cbor_message<T>(stream: &mut Stream, message: T) -> Result<()>
where
    T: Serialize,
{
    let wrapped = Ok::<_, SwapSetupError>(message);
    let bytes = serde_cbor::to_vec(&wrapped)
        .context("Failed to serialize message as bytes using CBOR")?;

    let mut frame = Framed::new(stream, codec());

    frame
        .send(Bytes::from(bytes))
        .await
        .context("Failed to write bytes as length-prefixed message")?;

    Ok(())
}

pub async fn write_cbor_error(stream: &mut Stream, error: SwapSetupError) -> Result<()> {
    let wrapped = Err::<(), _>(error);
    let bytes = serde_cbor::to_vec(&wrapped)
        .context("Failed to serialize error as bytes using CBOR")?;

    let mut frame = Framed::new(stream, codec());

    frame
        .send(Bytes::from(bytes))
        .await
        .context("Failed to write bytes as length-prefixed message")?;

    Ok(())
}
