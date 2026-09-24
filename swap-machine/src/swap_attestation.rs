use anyhow::{Context, Result, ensure};
use libp2p::{PeerId, identity};
use serde::{Deserialize, Serialize};
use swap_core::monero;
use uuid::Uuid;

const DOMAIN: &str = "xmr-btc-swap swap attestation v1";

/// A statement by the maker, signed with its libp2p identity, that it has
/// done a swap with the taker which progressed at least to the Bitcoin being locked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwapAttestation {
    pub swap: AttestedSwap,
    #[serde(with = "hex::serde")]
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttestedSwap {
    pub maker: PeerId,
    pub taker: PeerId,
    pub swap_id: Uuid,
    pub terms: SwapTerms,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwapTerms {
    #[serde(with = "::bitcoin::amount::serde::as_sat")]
    pub btc_amount: bitcoin::Amount,
    pub xmr_amount: monero::Amount,
    pub btc_lock_txid: bitcoin::Txid,
}

impl SwapAttestation {
    pub fn sign(swap: AttestedSwap, maker_identity: &identity::Keypair) -> Result<Self> {
        ensure!(
            swap.maker == maker_identity.public().to_peer_id(),
            "Swap attestation must be signed by the maker"
        );

        let signature = maker_identity
            .sign(swap.message().as_bytes())
            .context("Failed to sign swap attestation")?;

        Ok(Self { swap, signature })
    }

    /// Verifies the signature against the public key embedded in the maker's peer id.
    pub fn verify(&self) -> Result<()> {
        let maker_public_key =
            identity::PublicKey::try_decode_protobuf(self.swap.maker.as_ref().digest())
                .context("Maker peer id does not embed a public key")?;
        ensure!(
            maker_public_key.to_peer_id() == self.swap.maker,
            "Maker peer id does not match its embedded public key"
        );
        ensure!(
            maker_public_key.verify(self.swap.message().as_bytes(), &self.signature),
            "Invalid swap attestation signature"
        );

        Ok(())
    }
}

impl AttestedSwap {
    /// The canonical message the maker signs.
    pub fn message(&self) -> String {
        [
            DOMAIN.to_string(),
            format!("maker: {}", self.maker),
            format!("taker: {}", self.taker),
            format!("swap_id: {}", self.swap_id),
            format!("btc_lock_txid: {}", self.terms.btc_lock_txid),
            format!("btc_amount_sat: {}", self.terms.btc_amount.to_sat()),
            format!("xmr_amount_piconero: {}", self.terms.xmr_amount.as_pico()),
        ]
        .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::bitcoin::hashes::Hash;

    fn attested_swap(maker: &identity::Keypair) -> AttestedSwap {
        AttestedSwap {
            maker: maker.public().to_peer_id(),
            taker: PeerId::random(),
            swap_id: Uuid::new_v4(),
            terms: SwapTerms {
                btc_amount: bitcoin::Amount::from_sat(1_000_000),
                xmr_amount: monero::Amount::from_pico(2_000_000_000_000),
                btc_lock_txid: bitcoin::Txid::all_zeros(),
            },
        }
    }

    #[test]
    fn signed_attestation_verifies() {
        let maker = identity::Keypair::generate_ed25519();
        let attestation = SwapAttestation::sign(attested_swap(&maker), &maker).unwrap();

        attestation.verify().unwrap();
    }

    #[test]
    fn serde_roundtrip_preserves_validity() {
        let maker = identity::Keypair::generate_ed25519();
        let attestation = SwapAttestation::sign(attested_swap(&maker), &maker).unwrap();

        let json = serde_json::to_string(&attestation).unwrap();
        let decoded: SwapAttestation = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, attestation);
        decoded.verify().unwrap();
    }

    #[test]
    fn tampered_terms_fail_verification() {
        let maker = identity::Keypair::generate_ed25519();
        let mut attestation = SwapAttestation::sign(attested_swap(&maker), &maker).unwrap();
        attestation.swap.terms.btc_amount = bitcoin::Amount::from_sat(2_000_000);

        assert!(attestation.verify().is_err());
    }

    #[test]
    fn tampered_taker_fails_verification() {
        let maker = identity::Keypair::generate_ed25519();
        let mut attestation = SwapAttestation::sign(attested_swap(&maker), &maker).unwrap();
        attestation.swap.taker = PeerId::random();

        assert!(attestation.verify().is_err());
    }

    #[test]
    fn different_maker_fails_verification() {
        let maker = identity::Keypair::generate_ed25519();
        let mut attestation = SwapAttestation::sign(attested_swap(&maker), &maker).unwrap();
        attestation.swap.maker = identity::Keypair::generate_ed25519().public().to_peer_id();

        assert!(attestation.verify().is_err());
    }

    #[test]
    fn cannot_sign_for_another_maker() {
        let maker = identity::Keypair::generate_ed25519();
        let other = identity::Keypair::generate_ed25519();

        assert!(SwapAttestation::sign(attested_swap(&maker), &other).is_err());
    }
}
