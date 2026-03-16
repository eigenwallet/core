#![warn(
    unused_extern_crates,
    missing_copy_implementations,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::fallible_impl_from,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::dbg_macro
)]
#![forbid(unsafe_code)]
#![allow(non_snake_case)]

use anyhow::Result;
use std::env;
use swap::cli::command::{parse_args_and_apply_defaults, ParseResult};

#[tokio::main]
pub async fn main() -> Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("failed to install default rustls provider");

    match parse_args_and_apply_defaults(env::args_os()).await? {
        ParseResult::Success(context) => {
            context.tasks.wait_for_tasks().await?;
        }
        ParseResult::PrintAndExitZero { message } => {
            println!("{}", message);
            std::process::exit(0);
        }
    };

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::bitcoin::Amount;
    use bitcoin::address::NetworkUnchecked;
    use bitcoin::Address;
    use futures::future::{BoxFuture, FutureExt};
    use libp2p::PeerId;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use swap::cli::api::request::determine_btc_to_swap;
    use swap::cli::QuoteWithAddress;
    use swap::network::quote::{BidQuote, RefundPolicyWire};
    use tracing::level_filters::LevelFilter;
    use tracing_ext::capture_logs;

    const SWAP_ID: &str = "ea030832-3be9-454f-bb98-5ea9a788406b";

    #[tokio::test]
    async fn given_no_balance_and_transfers_less_than_max_swaps_max_giveable() {
        let writer = capture_logs(LevelFilter::INFO);
        let givable = MaxGiveable::new(vec![
            (Amount::ZERO, Amount::from_sat(1000)),
            (Amount::from_btc(0.0009).unwrap(), Amount::from_sat(1000)),
        ]);

        let (.., amount, fees) = determine_btc_to_swap(
            quote_with_max(0.01),
            get_dummy_address(),
            || async { Ok(Amount::from_btc(0.001)?) },
            givable.into_max_giveable_fn(),
            || async { Ok(()) },
            None,
            SWAP_ID.parse().unwrap(),
            approval_always,
        )
        .await
        .unwrap();

        let expected_amount = Amount::from_btc(0.0009).unwrap();
        let expected_fees = Amount::from_btc(0.0001).unwrap();

        assert_eq!((amount, fees), (expected_amount, expected_fees));
        assert_eq!(
            writer.captured(),
            r" INFO swap::cli::api::request: Received quote price=0.00100000 BTC minimum_amount=0 BTC maximum_amount=0.01000000 BTC
 INFO swap::cli::api::request: Deposit at least 0.00001000 BTC to cover the min quantity with fee!
 INFO swap::cli::api::request: Waiting for Bitcoin deposit deposit_address=1PdfytjS7C8wwd9Lq5o4x9aXA2YRqaCpH6 min_deposit_until_swap_will_start=0.00001000 BTC max_deposit_until_maximum_amount_is_reached=0.01001000 BTC max_giveable=0 BTC minimum_amount=0 BTC maximum_amount=0.01000000 BTC min_bitcoin_lock_tx_fee=0.00001000 BTC price=0.00100000 BTC
 INFO swap::cli::api::request: Received Bitcoin new_balance=0.00100000 BTC max_giveable=0.00090000 BTC
"
        );
    }

    #[tokio::test]
    async fn given_no_balance_and_transfers_more_then_swaps_max_quantity_from_quote() {
        let writer = capture_logs(LevelFilter::INFO);
        let givable = MaxGiveable::new(vec![
            (Amount::ZERO, Amount::from_sat(1000)),
            (Amount::from_btc(0.1).unwrap(), Amount::from_sat(1000)),
        ]);

        let (.., amount, fees) = determine_btc_to_swap(
            quote_with_max(0.01),
            get_dummy_address(),
            || async { Ok(Amount::from_btc(0.1001)?) },
            givable.into_max_giveable_fn(),
            || async { Ok(()) },
            None,
            SWAP_ID.parse().unwrap(),
            approval_always,
        )
        .await
        .unwrap();

        let expected_amount = Amount::from_btc(0.01).unwrap();
        let expected_fees = Amount::from_btc(0.0001).unwrap();

        assert_eq!((amount, fees), (expected_amount, expected_fees));
        assert_eq!(
            writer.captured(),
            r" INFO swap::cli::api::request: Received quote price=0.00100000 BTC minimum_amount=0 BTC maximum_amount=0.01000000 BTC
 INFO swap::cli::api::request: Deposit at least 0.00001000 BTC to cover the min quantity with fee!
 INFO swap::cli::api::request: Waiting for Bitcoin deposit deposit_address=1PdfytjS7C8wwd9Lq5o4x9aXA2YRqaCpH6 min_deposit_until_swap_will_start=0.00001000 BTC max_deposit_until_maximum_amount_is_reached=0.01001000 BTC max_giveable=0 BTC minimum_amount=0 BTC maximum_amount=0.01000000 BTC min_bitcoin_lock_tx_fee=0.00001000 BTC price=0.00100000 BTC
 INFO swap::cli::api::request: Received Bitcoin new_balance=0.10010000 BTC max_giveable=0.10000000 BTC
"
        );
    }

    #[tokio::test]
    async fn given_initial_balance_below_max_quantity_swaps_max_giveable() {
        let writer = capture_logs(LevelFilter::INFO);
        let givable = MaxGiveable::new(vec![
            (Amount::from_btc(0.0049).unwrap(), Amount::from_sat(1000)),
            (Amount::from_btc(99.9).unwrap(), Amount::from_sat(1000)),
        ]);

        let (.., amount, fees) = determine_btc_to_swap(
            quote_with_max(0.01),
            async { panic!("should not request new address when initial balance is > 0") },
            || async { Ok(Amount::from_btc(0.005)?) },
            givable.into_max_giveable_fn(),
            || async { Ok(()) },
            None,
            SWAP_ID.parse().unwrap(),
            approval_always,
        )
        .await
        .unwrap();

        let expected_amount = Amount::from_btc(0.0049).unwrap();
        let expected_fees = Amount::from_btc(0.0001).unwrap();

        assert_eq!((amount, fees), (expected_amount, expected_fees));
        assert_eq!(
            writer.captured(),
            " INFO swap::cli::api::request: Received quote price=0.00100000 BTC minimum_amount=0 BTC maximum_amount=0.01000000 BTC\n"
        );
    }

    #[tokio::test]
    async fn given_initial_balance_above_max_quantity_swaps_max_quantity() {
        let writer = capture_logs(LevelFilter::INFO);
        let givable = MaxGiveable::new(vec![
            (Amount::from_btc(0.1).unwrap(), Amount::from_sat(1000)),
            (Amount::from_btc(99.9).unwrap(), Amount::from_sat(1000)),
        ]);

        let (.., amount, fees) = determine_btc_to_swap(
            quote_with_max(0.01),
            async { panic!("should not request new address when initial balance is > 0") },
            || async { Ok(Amount::from_btc(0.1001)?) },
            givable.into_max_giveable_fn(),
            || async { Ok(()) },
            None,
            SWAP_ID.parse().unwrap(),
            approval_always,
        )
        .await
        .unwrap();

        let expected_amount = Amount::from_btc(0.01).unwrap();
        let expected_fees = Amount::from_btc(0.0001).unwrap();

        assert_eq!((amount, fees), (expected_amount, expected_fees));
        assert_eq!(
            writer.captured(),
            " INFO swap::cli::api::request: Received quote price=0.00100000 BTC minimum_amount=0 BTC maximum_amount=0.01000000 BTC\n"
        );
    }

    #[tokio::test]
    async fn given_no_initial_balance_then_min_wait_for_sufficient_deposit() {
        let writer = capture_logs(LevelFilter::INFO);
        let givable = MaxGiveable::new(vec![
            (Amount::ZERO, Amount::from_sat(1000)),
            (Amount::from_btc(0.01).unwrap(), Amount::from_sat(1000)),
        ]);

        let (.., amount, fees) = determine_btc_to_swap(
            quote_with_min(0.01),
            get_dummy_address(),
            || async { Ok(Amount::from_btc(0.0101)?) },
            givable.into_max_giveable_fn(),
            || async { Ok(()) },
            None,
            SWAP_ID.parse().unwrap(),
            approval_always,
        )
        .await
        .unwrap();

        let expected_amount = Amount::from_btc(0.01).unwrap();
        let expected_fees = Amount::from_btc(0.0001).unwrap();

        assert_eq!((amount, fees), (expected_amount, expected_fees));
        assert_eq!(
            writer.captured(),
            r" INFO swap::cli::api::request: Received quote price=0.00100000 BTC minimum_amount=0.01000000 BTC maximum_amount=21000000 BTC
 INFO swap::cli::api::request: Deposit at least 0.01001000 BTC to cover the min quantity with fee!
 INFO swap::cli::api::request: Waiting for Bitcoin deposit deposit_address=1PdfytjS7C8wwd9Lq5o4x9aXA2YRqaCpH6 min_deposit_until_swap_will_start=0.01001000 BTC max_deposit_until_maximum_amount_is_reached=21000000.00001000 BTC max_giveable=0 BTC minimum_amount=0.01000000 BTC maximum_amount=21000000 BTC min_bitcoin_lock_tx_fee=0.00001000 BTC price=0.00100000 BTC
 INFO swap::cli::api::request: Received Bitcoin new_balance=0.01010000 BTC max_giveable=0.01000000 BTC
"
        );
    }

    #[tokio::test]
    async fn given_balance_less_then_min_wait_for_sufficient_deposit() {
        let writer = capture_logs(LevelFilter::INFO);
        let givable = MaxGiveable::new(vec![
            (Amount::from_btc(0.0001).unwrap(), Amount::from_sat(1000)),
            (Amount::from_btc(0.01).unwrap(), Amount::from_sat(1000)),
        ]);

        let (.., amount, fees) = determine_btc_to_swap(
            quote_with_min(0.01),
            get_dummy_address(),
            || async { Ok(Amount::from_btc(0.0101)?) },
            givable.into_max_giveable_fn(),
            || async { Ok(()) },
            None,
            SWAP_ID.parse().unwrap(),
            approval_always,
        )
        .await
        .unwrap();

        let expected_amount = Amount::from_btc(0.01).unwrap();
        let expected_fees = Amount::from_btc(0.0001).unwrap();

        assert_eq!((amount, fees), (expected_amount, expected_fees));
        assert_eq!(
            writer.captured(),
            r" INFO swap::cli::api::request: Received quote price=0.00100000 BTC minimum_amount=0.01000000 BTC maximum_amount=21000000 BTC
 INFO swap::cli::api::request: Deposit at least 0.00991000 BTC to cover the min quantity with fee!
 INFO swap::cli::api::request: Waiting for Bitcoin deposit deposit_address=1PdfytjS7C8wwd9Lq5o4x9aXA2YRqaCpH6 min_deposit_until_swap_will_start=0.00991000 BTC max_deposit_until_maximum_amount_is_reached=20999999.99991000 BTC max_giveable=0.00010000 BTC minimum_amount=0.01000000 BTC maximum_amount=21000000 BTC min_bitcoin_lock_tx_fee=0.00001000 BTC price=0.00100000 BTC
 INFO swap::cli::api::request: Received Bitcoin new_balance=0.01010000 BTC max_giveable=0.01000000 BTC
"
        );
    }

    #[tokio::test]
    async fn given_no_initial_balance_and_transfers_less_than_min_keep_waiting() {
        let writer = capture_logs(LevelFilter::INFO);
        let givable = MaxGiveable::new(vec![
            (Amount::ZERO, Amount::from_sat(1000)),
            (Amount::from_btc(0.01).unwrap(), Amount::from_sat(1000)),
            (Amount::from_btc(0.01).unwrap(), Amount::from_sat(1000)),
            (Amount::from_btc(0.01).unwrap(), Amount::from_sat(1000)),
            (Amount::from_btc(0.01).unwrap(), Amount::from_sat(1000)),
        ]);

        let error = tokio::time::timeout(
            Duration::from_secs(1),
            determine_btc_to_swap(
                quote_with_min(0.1),
                get_dummy_address(),
                || async { Ok(Amount::from_btc(0.0101)?) },
                givable.into_max_giveable_fn(),
                || async { Ok(()) },
                None,
                SWAP_ID.parse().unwrap(),
                approval_always,
            ),
        )
        .await
        .unwrap_err();

        assert!(matches!(error, tokio::time::error::Elapsed { .. }));
        assert_eq!(
            writer.captured(),
            r" INFO swap::cli::api::request: Received quote price=0.00100000 BTC minimum_amount=0.10000000 BTC maximum_amount=21000000 BTC
 INFO swap::cli::api::request: Deposit at least 0.10001000 BTC to cover the min quantity with fee!
 INFO swap::cli::api::request: Waiting for Bitcoin deposit deposit_address=1PdfytjS7C8wwd9Lq5o4x9aXA2YRqaCpH6 min_deposit_until_swap_will_start=0.10001000 BTC max_deposit_until_maximum_amount_is_reached=21000000.00001000 BTC max_giveable=0 BTC minimum_amount=0.10000000 BTC maximum_amount=21000000 BTC min_bitcoin_lock_tx_fee=0.00001000 BTC price=0.00100000 BTC
 INFO swap::cli::api::request: Received Bitcoin new_balance=0.01010000 BTC max_giveable=0.01000000 BTC
 INFO swap::cli::api::request: Deposited amount is not enough to cover `min_quantity` when accounting for network fees
 INFO swap::cli::api::request: Deposit at least 0.09001000 BTC to cover the min quantity with fee!
 INFO swap::cli::api::request: Waiting for Bitcoin deposit deposit_address=1PdfytjS7C8wwd9Lq5o4x9aXA2YRqaCpH6 min_deposit_until_swap_will_start=0.09001000 BTC max_deposit_until_maximum_amount_is_reached=20999999.99001000 BTC max_giveable=0.01000000 BTC minimum_amount=0.10000000 BTC maximum_amount=21000000 BTC min_bitcoin_lock_tx_fee=0.00001000 BTC price=0.00100000 BTC
"
        );
    }

    #[tokio::test]
    async fn given_longer_delay_until_deposit_should_not_spam_user() {
        let writer = capture_logs(LevelFilter::INFO);
        let givable = MaxGiveable::new(vec![
            (Amount::ZERO, Amount::from_sat(1000)),
            (Amount::ZERO, Amount::from_sat(1000)),
            (Amount::ZERO, Amount::from_sat(1000)),
            (Amount::ZERO, Amount::from_sat(1000)),
            (Amount::ZERO, Amount::from_sat(1000)),
            (Amount::ZERO, Amount::from_sat(1000)),
            (Amount::ZERO, Amount::from_sat(1000)),
            (Amount::ZERO, Amount::from_sat(1000)),
            (Amount::ZERO, Amount::from_sat(1000)),
            (Amount::from_btc(0.2).unwrap(), Amount::from_sat(1000)),
        ]);

        tokio::time::timeout(
            Duration::from_secs(10),
            determine_btc_to_swap(
                quote_with_min(0.1),
                get_dummy_address(),
                || async { Ok(Amount::from_btc(0.21)?) },
                givable.into_max_giveable_fn(),
                || async { Ok(()) },
                None,
                SWAP_ID.parse().unwrap(),
                approval_always,
            ),
        )
        .await
        .unwrap()
        .unwrap();

        assert_eq!(
            writer.captured(),
            r" INFO swap::cli::api::request: Received quote price=0.00100000 BTC minimum_amount=0.10000000 BTC maximum_amount=21000000 BTC
 INFO swap::cli::api::request: Deposit at least 0.10001000 BTC to cover the min quantity with fee!
 INFO swap::cli::api::request: Waiting for Bitcoin deposit deposit_address=1PdfytjS7C8wwd9Lq5o4x9aXA2YRqaCpH6 min_deposit_until_swap_will_start=0.10001000 BTC max_deposit_until_maximum_amount_is_reached=21000000.00001000 BTC max_giveable=0 BTC minimum_amount=0.10000000 BTC maximum_amount=21000000 BTC min_bitcoin_lock_tx_fee=0.00001000 BTC price=0.00100000 BTC
 INFO swap::cli::api::request: Received Bitcoin new_balance=0.21000000 BTC max_giveable=0.20000000 BTC
"
        );
    }

    #[tokio::test]
    async fn given_bid_quote_max_amount_0_return_error() {
        let givable = MaxGiveable::new(vec![
            (Amount::from_btc(0.0001).unwrap(), Amount::from_sat(1000)),
            (Amount::from_btc(0.01).unwrap(), Amount::from_sat(1000)),
        ]);

        let determination_error = determine_btc_to_swap(
            quote_with_max(0.00),
            get_dummy_address(),
            || async { Ok(Amount::from_btc(0.0101)?) },
            givable.into_max_giveable_fn(),
            || async { Ok(()) },
            None,
            SWAP_ID.parse().unwrap(),
            approval_always,
        )
        .await
        .unwrap_err()
        .to_string();

        assert_eq!("Received quote of 0", determination_error);
    }

    struct MaxGiveable {
        amounts: Vec<(Amount, Amount)>,
        call_counter: usize,
    }

    impl MaxGiveable {
        fn new(amounts: Vec<(Amount, Amount)>) -> Self {
            Self {
                amounts,
                call_counter: 0,
            }
        }
        fn give(&mut self) -> Result<(Amount, Amount)> {
            let amount = self
                .amounts
                .get(self.call_counter)
                .ok_or_else(|| anyhow::anyhow!("No more balances available"))?;
            self.call_counter += 1;
            Ok(*amount)
        }

        fn into_max_giveable_fn(self) -> impl Fn() -> BoxFuture<'static, Result<(Amount, Amount)>> {
            let givable = Arc::new(Mutex::new(self));
            move || {
                {
                    let givable = givable.clone();
                    async move {
                        let mut result = givable.lock().unwrap();
                        result.give()
                    }
                }
                .boxed()
            }
        }
    }

    fn quote_with_max(btc: f64) -> ::tokio::sync::watch::Receiver<Vec<QuoteWithAddress>> {
        quote_minmax(None, Some(btc))
    }

    fn quote_with_min(btc: f64) -> ::tokio::sync::watch::Receiver<Vec<QuoteWithAddress>> {
        quote_minmax(Some(btc), None)
    }

    fn quote_minmax(
        min: Option<f64>,
        max: Option<f64>,
    ) -> ::tokio::sync::watch::Receiver<Vec<QuoteWithAddress>> {
        let max_quantity = max
            .map(|m| Amount::from_btc(m).unwrap())
            .unwrap_or(Amount::MAX_MONEY);
        let min_quantity = min
            .map(|m| Amount::from_btc(m).unwrap())
            .unwrap_or(Amount::ZERO);

        let (_, rx) = ::tokio::sync::watch::channel(vec![QuoteWithAddress {
            peer_id: PeerId::random(),
            multiaddr: "/ip4/127.0.0.1/tcp/5678".parse().unwrap(),
            quote: BidQuote {
                price: Amount::from_btc(0.001).unwrap(),
                max_quantity,
                min_quantity,
                refund_policy: RefundPolicyWire::FullRefund,
                reserve_proof: None,
            },
            version: Some("1.0.0".parse().unwrap()),
        }]);
        rx
    }

    async fn get_dummy_address() -> Result<bitcoin::Address> {
        Ok("1PdfytjS7C8wwd9Lq5o4x9aXA2YRqaCpH6"
            .parse::<Address<NetworkUnchecked>>()?
            .assume_checked())
    }

    fn approval_always(
        qwa: QuoteWithAddress,
    ) -> Box<dyn std::future::Future<Output = Result<bool>> + Send> {
        dbg!(qwa);
        Box::new(async { Ok(true) })
    }
}
