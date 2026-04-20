//! End-to-end decryption tests against `.keys` files produced by
//! `monero-wallet-rpc` via `create_wallet` + `query_key`. Each fixture has
//! a `.keys` ciphertext and a sibling `.json` with the expected ground truth.
//!
//! Regenerate with `tests/fixtures/regenerate.sh` (or the dev script at the
//! crate root) whenever the RPC-generated format changes.

use std::fs;
use std::path::Path;

use serde::Deserialize;

const FIXTURE_NAMES: &[&str] = &["wallet_empty", "wallet_short", "wallet_long"];

#[derive(Deserialize)]
struct Expected {
    password: String,
    spend_secret_key: String,
    view_secret_key: String,
}

fn load_expected(dir: &Path, name: &str) -> Expected {
    let path = dir.join(format!("{name}.json"));
    serde_json::from_slice(&fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display())))
        .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

fn fixtures_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

#[test]
fn decrypts_all_rpc_generated_fixtures() {
    let dir = fixtures_dir();

    for name in FIXTURE_NAMES {
        let keys_path = dir.join(format!("{name}.keys"));
        let expected = load_expected(&dir, name);

        let keys = monero_wallet2_adapter::load_wallet_keys(
            keys_path.to_str().expect("utf-8 fixture path"),
            &expected.password,
            1,
        )
        .unwrap_or_else(|e| panic!("decrypt {}: {e}", keys_path.display()));

        assert_eq!(
            hex::encode(keys.spend_secret_key),
            expected.spend_secret_key,
            "{name}: spend_secret_key mismatch"
        );
        assert_eq!(
            hex::encode(keys.view_secret_key),
            expected.view_secret_key,
            "{name}: view_secret_key mismatch"
        );
    }
}

#[test]
fn rejects_wrong_password() {
    let dir = fixtures_dir();

    for name in FIXTURE_NAMES {
        let keys_path = dir.join(format!("{name}.keys"));
        let expected = load_expected(&dir, name);
        let wrong = "definitely-not-the-real-password";

        assert_ne!(
            wrong, expected.password,
            "{name}: pick a different 'wrong' password in the test"
        );

        let result = monero_wallet2_adapter::load_wallet_keys(
            keys_path.to_str().expect("utf-8 fixture path"),
            wrong,
            1,
        );

        assert!(
            result.is_err(),
            "{name}: decryption with wrong password unexpectedly succeeded"
        );
    }
}
