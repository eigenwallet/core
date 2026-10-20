use anyhow::{Context, Result};
use asynchronous_codec::{Bytes, Framed};
use futures::{SinkExt, StreamExt};

use libp2p::swarm::Stream;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub mod alice;
pub mod bob;
mod vendor_from_fn;
pub mod verification;

pub const BUF_SIZE: usize = 1024 * 1024;

pub mod protocol {
    use futures::future;
    use libp2p::core::Endpoint;
    use libp2p::swarm::Stream;
    use void::Void;

    use super::vendor_from_fn::{FromFnUpgrade, from_fn};

    pub fn new() -> SwapSetup {
        from_fn(
            "/comit/xmr/btc/swap_setup/4.0.0",
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum SwapSetupError {
    #[error("Invalid swap fee proposal: {0}")]
    InvalidFeeProposal(String),
    #[error("Bitcoin lock transaction has too many inputs ({actual}, maximum: {maximum})")]
    TxLockTooManyInputs { actual: u32, maximum: u32 },
    #[error("Input verification failed.")]
    LockInputVerificationFailed,
    #[error("Invalid DLEQ proof")]
    InvalidDleqProof,
    #[error("Anti-spam deposit {deposit} is too small; minimum required is {minimum}")]
    AntiSpamDepositTooSmall {
        deposit: bitcoin::Amount,
        minimum: bitcoin::Amount,
    },
    #[error("Anti-spam deposit ratio {ratio} exceeds maximum {max}")]
    AntiSpamDepositRatioTooHigh {
        ratio: rust_decimal::Decimal,
        max: rust_decimal::Decimal,
    },
    #[error("Swap spending path fees ({fees}) exceed local budget ({budget})")]
    SwapFeesExceedBudget {
        fees: bitcoin::Amount,
        budget: bitcoin::Amount,
    },
    #[error("Transaction fee is too low compared to our estimate ({fee} vs {estimate})")]
    TransactionFeeTooLow {
        fee: bitcoin::Amount,
        estimate: bitcoin::Amount,
    },
}

impl TryFrom<anyhow::Error> for SwapSetupError {
    type Error = anyhow::Error;

    fn try_from(error: anyhow::Error) -> Result<Self> {
        use swap_core::bitcoin::SwapFeeError;
        use swap_machine::common::SetupValidationError;

        // Only send explicit validation failures. Wallet and other local errors
        // retain their context locally and propagate through the retry path.
        if let Some(validation) = error.downcast_ref::<SwapFeeError>() {
            return Ok(match *validation {
                SwapFeeError::RateOutsideLimits { .. } => {
                    Self::InvalidFeeProposal(validation.to_string())
                }
                SwapFeeError::AntiSpamDepositTooSmall { deposit, minimum } => {
                    Self::AntiSpamDepositTooSmall { deposit, minimum }
                }
                SwapFeeError::SwapFeesExceedBudget { fees, budget } => {
                    Self::SwapFeesExceedBudget { fees, budget }
                }
            });
        }
        if let Some(validation) = error.downcast_ref::<SetupValidationError>() {
            return Ok(match *validation {
                SetupValidationError::InvalidDleqProof => Self::InvalidDleqProof,
                SetupValidationError::AntiSpamDepositRatioTooHigh { ratio, max } => {
                    Self::AntiSpamDepositRatioTooHigh { ratio, max }
                }
                SetupValidationError::TransactionFeeTooLow { fee, estimate } => {
                    Self::TransactionFeeTooLow { fee, estimate }
                }
            });
        }
        Err(error)
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
    let bytes =
        serde_cbor::to_vec(&wrapped).context("Failed to serialize message as bytes using CBOR")?;

    let mut frame = Framed::new(stream, codec());

    frame
        .send(Bytes::from(bytes))
        .await
        .context("Failed to write bytes as length-prefixed message")?;

    Ok(())
}

pub async fn write_cbor_error(stream: &mut Stream, error: SwapSetupError) -> Result<()> {
    let wrapped = Err::<(), _>(error);
    let bytes =
        serde_cbor::to_vec(&wrapped).context("Failed to serialize error as bytes using CBOR")?;

    let mut frame = Framed::new(stream, codec());

    frame
        .send(Bytes::from(bytes))
        .await
        .context("Failed to write bytes as length-prefixed message")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::Amount;
    use rust_decimal::Decimal;
    use swap_core::bitcoin::{SwapFeeError, SwapFeeRate};
    use swap_machine::common::SetupValidationError;

    #[test]
    fn validation_errors_keep_their_kind_and_values_on_the_wire() {
        let deposit = Amount::ONE_SAT;
        let minimum = Amount::from_sat(656);
        let ratio = Decimal::new(3, 1);
        let max = Decimal::new(2, 1);
        let fees = Amount::from_sat(100_001);
        let budget = Amount::from_sat(100_000);
        let rate_error = SwapFeeError::RateOutsideLimits {
            rate: SwapFeeRate(999),
            min: SwapFeeRate(1000),
            max: SwapFeeRate(3000),
        };
        let cases: Vec<(anyhow::Error, SwapSetupError)> = vec![
            (
                rate_error.into(),
                SwapSetupError::InvalidFeeProposal(rate_error.to_string()),
            ),
            (
                SetupValidationError::InvalidDleqProof.into(),
                SwapSetupError::InvalidDleqProof,
            ),
            (
                SwapFeeError::AntiSpamDepositTooSmall { deposit, minimum }.into(),
                SwapSetupError::AntiSpamDepositTooSmall { deposit, minimum },
            ),
            (
                SetupValidationError::AntiSpamDepositRatioTooHigh { ratio, max }.into(),
                SwapSetupError::AntiSpamDepositRatioTooHigh { ratio, max },
            ),
            (
                SwapFeeError::SwapFeesExceedBudget { fees, budget }.into(),
                SwapSetupError::SwapFeesExceedBudget { fees, budget },
            ),
            (
                SetupValidationError::TransactionFeeTooLow {
                    fee: deposit,
                    estimate: minimum,
                }
                .into(),
                SwapSetupError::TransactionFeeTooLow {
                    fee: deposit,
                    estimate: minimum,
                },
            ),
        ];
        for (error, expected) in cases {
            let rejection = SwapSetupError::try_from(error.context("local-only context")).unwrap();
            assert_eq!(rejection, expected);
            let bytes = serde_cbor::to_vec(&Err::<(), _>(rejection)).unwrap();
            let decoded: Result<(), SwapSetupError> = serde_cbor::from_slice(&bytes).unwrap();
            assert_eq!(decoded.unwrap_err(), expected);
            assert!(!expected.to_string().contains("local-only context"));
        }
    }

    #[test]
    fn local_errors_are_not_protocol_rejections() {
        let error = anyhow::Error::new(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "private Electrum server details",
        ))
        .context("Failed to build lock transaction");
        let local_error = SwapSetupError::try_from(error).unwrap_err();
        assert_eq!(
            local_error.downcast_ref::<std::io::Error>().unwrap().kind(),
            std::io::ErrorKind::TimedOut
        );
        assert!(format!("{local_error:#}").contains("private Electrum server details"));
    }
}
