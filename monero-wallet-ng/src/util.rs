//! Small shared helpers used across this crate and its tests.

use monero_oxide::ed25519::{Point, Scalar};

/// Derive the Ed25519 public key for a private scalar.
pub fn public_key(private_key: &Scalar) -> Point {
    Point::from(curve25519_dalek::constants::ED25519_BASEPOINT_POINT * (*private_key).into())
}
