use rust_decimal::Decimal;
use swap_env::config::Maker;

fn maker(options: &str) -> Result<Maker, toml::de::Error> {
    toml::from_str(&format!(
        "min_buy_btc = 0.001\nmax_buy_btc = 1.0\nask_spread = 0.01\n{options}"
    ))
}

#[test]
fn swap_fee_multiplier_defaults_and_accepts_the_new_name() {
    assert_eq!(
        maker("").unwrap().btc_swap_fee_rate_multiplier,
        Decimal::ONE
    );
    assert_eq!(
        maker("btc_swap_fee_rate_multiplier = 2.5")
            .unwrap()
            .btc_swap_fee_rate_multiplier,
        Decimal::new(25, 1)
    );
}

#[test]
fn old_redeem_fee_multiplier_is_rejected() {
    let error = maker("btc_redeem_fee_multiplier = 2.5").unwrap_err();
    assert!(error.to_string().contains("unknown field"));
    assert!(error.to_string().contains("btc_redeem_fee_multiplier"));
}

#[test]
fn redeem_base_fee_multiplier_is_an_exact_quoted_decimal() {
    assert_eq!(
        maker("").unwrap().btc_redeem_base_fee_multiplier,
        Decimal::from(2)
    );
    assert_eq!(
        maker(r#"btc_redeem_base_fee_multiplier = "2.30""#)
            .unwrap()
            .btc_redeem_base_fee_multiplier,
        Decimal::new(230, 2)
    );
    assert!(maker("btc_redeem_base_fee_multiplier = 2.30").is_err());
    assert!(
        maker(r#"btc_redeem_base_fee_multiplier = "2.30000000000000000000000000001""#).is_err()
    );
}
