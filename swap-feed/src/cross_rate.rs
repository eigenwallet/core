use bitcoin::amount::ParseAmountError;
use rust_decimal::{Decimal, RoundingStrategy};

const BTC_DECIMAL_PLACES: u32 = 8;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Price is not positive: {0}")]
    NonPositive(Decimal),
    #[error("Cross rate overflowed")]
    Overflow,
    #[error("Failed to parse cross rate {rate} as a Bitcoin amount")]
    AmountParse {
        rate: Decimal,
        #[source]
        source: ParseAmountError,
    },
}

/// The price of an asset in BTC, rounded up to whole satoshis.
/// Both input prices must be in the same quote currency.
pub(crate) fn cross_rate(
    asset_price: Decimal,
    btc_price: Decimal,
) -> Result<bitcoin::Amount, Error> {
    for price in [asset_price, btc_price] {
        if price <= Decimal::ZERO {
            return Err(Error::NonPositive(price));
        }
    }

    let rate = asset_price
        .checked_div(btc_price)
        .ok_or(Error::Overflow)?
        .round_dp_with_strategy(BTC_DECIMAL_PLACES, RoundingStrategy::ToPositiveInfinity);

    bitcoin::Amount::from_str_in(&rate.to_string(), bitcoin::Denomination::Bitcoin)
        .map_err(|source| Error::AmountParse { rate, source })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn decimal(value: &str) -> Decimal {
        Decimal::from_str(value).unwrap()
    }

    #[test]
    fn rounds_up_to_whole_satoshis() {
        // 441.13 / 59242.21 = 0.00744621...
        let rate = cross_rate(decimal("441.13"), decimal("59242.21")).unwrap();
        assert_eq!(rate.to_sat(), 744_622);
    }

    #[test]
    fn keeps_exact_rate() {
        let rate = cross_rate(decimal("500"), decimal("50000")).unwrap();
        assert_eq!(rate.to_sat(), 1_000_000);
    }

    #[test]
    fn rejects_zero_asset_price() {
        assert!(matches!(
            cross_rate(Decimal::ZERO, decimal("59242.21")),
            Err(Error::NonPositive(_))
        ));
    }

    #[test]
    fn rejects_zero_btc_price() {
        assert!(matches!(
            cross_rate(decimal("441.13"), Decimal::ZERO),
            Err(Error::NonPositive(_))
        ));
    }

    #[test]
    fn rejects_negative_price() {
        assert!(matches!(
            cross_rate(decimal("-441.13"), decimal("59242.21")),
            Err(Error::NonPositive(_))
        ));
    }
}
