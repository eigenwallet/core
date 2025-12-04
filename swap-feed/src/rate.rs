use anyhow::{Context, Result};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::convert::Infallible;
use std::fmt::{Debug, Display, Formatter};
use std::time::{Duration, Instant};

/// Represents the rate at which we are willing to trade 1 XMR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rate {
    /// Represents the asking price from the market.
    ask: bitcoin::Amount,
    /// The spread which should be applied to the market asking price.
    ask_spread: Decimal,
}

const ZERO_SPREAD: Decimal = Decimal::ZERO;

impl Rate {
    pub const ZERO: Rate = Rate {
        ask: bitcoin::Amount::ZERO,
        ask_spread: ZERO_SPREAD,
    };

    pub fn new(ask: bitcoin::Amount, ask_spread: Decimal) -> Self {
        Self { ask, ask_spread }
    }

    /// Computes the asking price at which we are willing to sell 1 XMR.
    ///
    /// This applies the spread to the market asking price.
    pub fn ask(&self) -> Result<bitcoin::Amount> {
        let sats = self.ask.to_sat();
        let sats = Decimal::from(sats);

        let additional_sats = sats * self.ask_spread;
        let additional_sats = bitcoin::Amount::from_sat(
            additional_sats
                .to_u64()
                .context("Failed to fit spread into u64")?,
        );

        Ok(self.ask + additional_sats)
    }

    /// Calculate a sell quote for a given BTC amount.
    pub fn sell_quote(&self, quote: bitcoin::Amount) -> Result<monero::Amount> {
        Self::quote(self.ask()?, quote)
    }

    fn quote(rate: bitcoin::Amount, quote: bitcoin::Amount) -> Result<monero::Amount> {
        // quote (btc) = rate * base (xmr)
        // base = quote / rate

        let quote_in_sats = quote.to_sat();
        let quote_in_btc = Decimal::from(quote_in_sats)
            .checked_div(Decimal::from(bitcoin::Amount::ONE_BTC.to_sat()))
            .context("Division overflow")?;

        let rate_in_btc = Decimal::from(rate.to_sat())
            .checked_div(Decimal::from(bitcoin::Amount::ONE_BTC.to_sat()))
            .context("Division overflow")?;

        let base_in_xmr = quote_in_btc
            .checked_div(rate_in_btc)
            .context("Division overflow")?;
        let base_in_piconero = base_in_xmr * Decimal::from(monero::Amount::ONE_XMR.as_pico());

        let base_in_piconero = base_in_piconero
            .to_u64()
            .context("Failed to fit piconero amount into a u64")?;

        Ok(monero::Amount::from_pico(base_in_piconero))
    }
}

impl Display for Rate {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.ask)
    }
}

#[derive(Clone, Debug)]
pub struct FixedRate(Rate);

impl FixedRate {
    pub const RATE: f64 = 0.01;

    pub fn value(&self) -> Rate {
        self.0
    }
}

impl Default for FixedRate {
    fn default() -> Self {
        let ask = bitcoin::Amount::from_btc(Self::RATE).expect("Static value should never fail");
        let spread = Decimal::from(0u64);

        Self(Rate::new(ask, spread))
    }
}

impl crate::traits::LatestRate for FixedRate {
    type Error = Infallible;

    fn latest_rate(&mut self) -> Result<Rate, Self::Error> {
        Ok(self.value())
    }
}

/// Produces [`Rate`]s based on [`PriceUpdate`]s from kraken, bitfinex, kucoin,
/// and a configured spread.
#[derive(Debug, Clone)]
pub struct ExchangeRate {
    ask_spread: Decimal,
    kraken_price_updates: crate::kraken::PriceUpdates,
    bitfinex_price_updates: crate::bitfinex::PriceUpdates,
    kucoin_price_updates: crate::kucoin::PriceUpdates,
}

impl ExchangeRate {
    pub fn new(
        ask_spread: Decimal,
        kraken_price_updates: crate::kraken::PriceUpdates,
        bitfinex_price_updates: crate::bitfinex::PriceUpdates,
        kucoin_price_updates: crate::kucoin::PriceUpdates,
    ) -> Self {
        Self {
            ask_spread,
            kraken_price_updates,
            bitfinex_price_updates,
            kucoin_price_updates,
        }
    }
}

#[derive(PartialEq, Clone, Debug, thiserror::Error)]
pub enum Error {
    #[error("All exchanges failed (Kraken: {0}, Bitfinex: {1}, KuCoin: {2})")]
    AllExchanges(
        crate::kraken::Error,
        crate::bitfinex::Error,
        crate::kucoin::Error,
    ),
    #[error("All exchange data is stale by >10 minutes")]
    AllStaleData,
    #[error("Exchanges disagree by more than 10%")]
    SpreadTooWide,
}

const MAX_INTEREXCHANGE_SPREAD: Decimal = Decimal::from_parts(1, 0, 0, false, 1); // 10%
const MAX_UPDATE_AGE: Duration = Duration::from_secs(10 * 60); // 10 minutes

impl crate::traits::LatestRate for ExchangeRate {
    type Error = Error;

    fn latest_rate(&mut self) -> Result<Rate, Self::Error> {
        let kraken_update = self.kraken_price_updates.latest_update();
        let bitfinex_update = self.bitfinex_price_updates.latest_update();
        let kucoin_update = self.kucoin_price_updates.latest_update();
        average_ask(kraken_update, bitfinex_update, kucoin_update)
            .map(|average_ask| Rate::new(average_ask, self.ask_spread))
    }
}

fn average_ask(
    kraken_update: crate::kraken::PriceUpdate,
    bitfinex_update: crate::bitfinex::PriceUpdate,
    kucoin_update: crate::kucoin::PriceUpdate,
) -> Result<bitcoin::Amount, Error> {
    if kraken_update.is_err() && bitfinex_update.is_err() && kucoin_update.is_err() {
        return Err(Error::AllExchanges(
            kraken_update.unwrap_err(),
            bitfinex_update.unwrap_err(),
            kucoin_update.unwrap_err(),
        ));
    }

    let now = Instant::now();
    let kraken_update = kraken_update.map(|(ts, u)| (now - ts, u.ask));
    let bitfinex_update = bitfinex_update.map(|(ts, u)| (now - ts, u.ask));
    let kucoin_update = kucoin_update.map(|(ts, u)| (now - ts, u.ask));
    let asks: Vec<_> = [
        kraken_update.as_ref().ok(),
        bitfinex_update.as_ref().ok(),
        kucoin_update.as_ref().ok(),
    ]
    .into_iter()
    .flatten()
    .filter(|(age, _)| *age <= MAX_UPDATE_AGE)
    .map(|(_, ask)| ask)
    .copied()
    .collect();
    if asks.is_empty() {
        return Err(Error::AllStaleData);
    }
    let degraded = asks.len() < 3;

    let average_ask = asks.iter().copied().sum::<bitcoin::Amount>() / (asks.len() as u64);
    let min_ask = asks.iter().min().expect(">0 asks");
    let max_ask = asks.iter().max().expect(">0 asks");
    assert!(*max_ask >= *min_ask, "bitcoin::Amount violates Ord");

    let spread = *max_ask - *min_ask;
    if degraded {
        tracing::warn!(?kraken_update, ?bitfinex_update, ?kucoin_update, %average_ask, %spread, %degraded, "Computing latest XMR/BTC rate");
    } else {
        tracing::debug!(?kraken_update, ?bitfinex_update, ?kucoin_update, %average_ask, %spread, %degraded, "Computing latest XMR/BTC rate");
    }

    if Decimal::from(spread.to_sat())
        > Decimal::from(average_ask.to_sat()) * MAX_INTEREXCHANGE_SPREAD
    {
        return Err(Error::SpreadTooWide);
    }

    Ok(average_ask)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO_PERCENT: Decimal = Decimal::from_parts(2, 0, 0, false, 2);
    const ONE: Decimal = Decimal::from_parts(1, 0, 0, false, 0);

    #[test]
    fn sell_quote() {
        let asking_price = bitcoin::Amount::from_btc(0.002_500).unwrap();
        let rate = Rate::new(asking_price, ZERO_SPREAD);

        let btc_amount = bitcoin::Amount::from_btc(2.5).unwrap();

        let xmr_amount = rate.sell_quote(btc_amount).unwrap();

        assert_eq!(xmr_amount, monero::Amount::from_xmr(1000.0).unwrap())
    }

    #[test]
    fn applies_spread_to_asking_price() {
        let asking_price = bitcoin::Amount::from_sat(100);
        let rate = Rate::new(asking_price, TWO_PERCENT);

        let amount = rate.ask().unwrap();

        assert_eq!(amount.to_sat(), 102);
    }

    #[test]
    fn given_spread_of_two_percent_when_caluclating_sell_quote_factor_between_should_be_two_percent(
    ) {
        let asking_price = bitcoin::Amount::from_btc(0.004).unwrap();

        let rate_no_spread = Rate::new(asking_price, ZERO_SPREAD);
        let rate_with_spread = Rate::new(asking_price, TWO_PERCENT);

        let xmr_no_spread = rate_no_spread.sell_quote(bitcoin::Amount::ONE_BTC).unwrap();
        let xmr_with_spread = rate_with_spread
            .sell_quote(bitcoin::Amount::ONE_BTC)
            .unwrap();

        let xmr_factor = Decimal::from_f64_retain(xmr_no_spread.as_pico() as _).unwrap()
            / Decimal::from_f64_retain(xmr_with_spread.as_pico() as _).unwrap()
            - ONE;

        assert!(xmr_with_spread < xmr_no_spread);
        assert_eq!(xmr_factor.round_dp(8), TWO_PERCENT); // round to 8 decimal
                                                         // places to show that
                                                         // it is really close
                                                         // to two percent
    }

    mod average_ask {
        use super::*;

        #[test]
        fn normal() {
            let now = Instant::now();
            let kraken_update = Ok((
                now,
                crate::kraken::wire::PriceUpdate {
                    ask: bitcoin::Amount::from_btc(0.95).unwrap(),
                },
            ));
            let bitfinex_update = Ok((
                now,
                crate::bitfinex::wire::PriceUpdate {
                    ask: bitcoin::Amount::from_btc(1.00).unwrap(),
                },
            ));
            let kucoin_update = Ok((
                now,
                crate::kucoin::wire::PriceUpdate {
                    ask: bitcoin::Amount::from_btc(1.05).unwrap(),
                },
            ));
            assert_eq!(
                average_ask(kraken_update, bitfinex_update, kucoin_update),
                Ok(bitcoin::Amount::ONE_BTC)
            );
        }

        #[test]
        fn err1() {
            let now = Instant::now();
            let kraken_update = Ok((
                now,
                crate::kraken::wire::PriceUpdate {
                    ask: bitcoin::Amount::from_btc(0.95).unwrap(),
                },
            ));
            let bitfinex_update = Err(crate::bitfinex::Error::NotYetAvailable);
            let kucoin_update = Ok((
                now,
                crate::kucoin::wire::PriceUpdate {
                    ask: bitcoin::Amount::from_btc(1.05).unwrap(),
                },
            ));
            assert_eq!(
                average_ask(kraken_update, bitfinex_update, kucoin_update),
                Ok(bitcoin::Amount::ONE_BTC)
            );
        }

        #[test]
        fn err2() {
            let now = Instant::now();
            let kraken_update = Err(crate::kraken::Error::NotYetAvailable);
            let bitfinex_update = Ok((
                now,
                crate::bitfinex::wire::PriceUpdate {
                    ask: bitcoin::Amount::from_btc(1.00).unwrap(),
                },
            ));
            let kucoin_update = Err(crate::kucoin::Error::NotYetAvailable);
            assert_eq!(
                average_ask(kraken_update, bitfinex_update, kucoin_update),
                Ok(bitcoin::Amount::ONE_BTC)
            );
        }

        #[test]
        fn err3() {
            let kraken_update = Err(crate::kraken::Error::NotYetAvailable);
            let bitfinex_update = Err(crate::bitfinex::Error::NotYetAvailable);
            let kucoin_update = Err(crate::kucoin::Error::NotYetAvailable);
            assert_eq!(
                average_ask(kraken_update, bitfinex_update, kucoin_update),
                Err(Error::AllExchanges(
                    crate::kraken::Error::NotYetAvailable,
                    crate::bitfinex::Error::NotYetAvailable,
                    crate::kucoin::Error::NotYetAvailable
                ))
            );
        }

        #[test]
        fn spread_too_wide() {
            let now = Instant::now();
            let kraken_update = Ok((
                now,
                crate::kraken::wire::PriceUpdate {
                    ask: bitcoin::Amount::from_btc(0.85).unwrap(),
                },
            ));
            let bitfinex_update = Ok((
                now,
                crate::bitfinex::wire::PriceUpdate {
                    ask: bitcoin::Amount::from_btc(1.00).unwrap(),
                },
            ));
            let kucoin_update = Ok((
                now,
                crate::kucoin::wire::PriceUpdate {
                    ask: bitcoin::Amount::from_btc(1.15).unwrap(),
                },
            ));
            assert_eq!(
                average_ask(kraken_update, bitfinex_update, kucoin_update),
                Err(Error::SpreadTooWide)
            );
        }

        #[test]
        fn old() {
            let now = Instant::now();
            let old = Instant::now() - MAX_UPDATE_AGE - Duration::from_secs(1);
            let kraken_update = Ok((
                now,
                crate::kraken::wire::PriceUpdate {
                    ask: bitcoin::Amount::from_btc(0.95).unwrap(),
                },
            ));
            let bitfinex_update = Ok((
                old,
                crate::bitfinex::wire::PriceUpdate {
                    ask: bitcoin::Amount::from_btc(200.00).unwrap(),
                },
            ));
            let kucoin_update = Ok((
                now,
                crate::kucoin::wire::PriceUpdate {
                    ask: bitcoin::Amount::from_btc(1.05).unwrap(),
                },
            ));
            assert_eq!(
                average_ask(kraken_update, bitfinex_update, kucoin_update),
                Ok(bitcoin::Amount::ONE_BTC)
            );
        }

        #[test]
        fn old_err() {
            let now = Instant::now();
            let old = Instant::now() - MAX_UPDATE_AGE - Duration::from_secs(1);
            let kraken_update = Err(crate::kraken::Error::NotYetAvailable);
            let bitfinex_update = Ok((
                old,
                crate::bitfinex::wire::PriceUpdate {
                    ask: bitcoin::Amount::from_btc(200.00).unwrap(),
                },
            ));
            let kucoin_update = Ok((
                now,
                crate::kucoin::wire::PriceUpdate {
                    ask: bitcoin::Amount::from_btc(1.00).unwrap(),
                },
            ));
            assert_eq!(
                average_ask(kraken_update, bitfinex_update, kucoin_update),
                Ok(bitcoin::Amount::ONE_BTC)
            );
        }

        #[test]
        fn old_and_dry() {
            let old = Instant::now() - MAX_UPDATE_AGE - Duration::from_secs(1);
            let kraken_update = Ok((
                old,
                crate::kraken::wire::PriceUpdate {
                    ask: bitcoin::Amount::from_btc(0.95).unwrap(),
                },
            ));
            let bitfinex_update = Ok((
                old,
                crate::bitfinex::wire::PriceUpdate {
                    ask: bitcoin::Amount::from_btc(1.00).unwrap(),
                },
            ));
            let kucoin_update = Ok((
                old,
                crate::kucoin::wire::PriceUpdate {
                    ask: bitcoin::Amount::from_btc(1.05).unwrap(),
                },
            ));
            assert_eq!(
                average_ask(kraken_update, bitfinex_update, kucoin_update),
                Err(Error::AllStaleData)
            );
        }
    }
}
