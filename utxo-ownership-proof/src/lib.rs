use bitcoin::hashes::{Hash, HashEngine, hash160, sha256};
use bitcoin::secp256k1::{Message, PublicKey, Secp256k1, SecretKey, ecdsa::Signature};
use serde::{Deserialize, Serialize};

/// Domain separation tag. Fixed by the crate.
const CHALLENGE_TAG: &str = "eigenwallet/utxo-ownership/v1";

/// A domain-separated digest, constructible only via [`challenge`].
///
/// Signing is restricted to this type so a proof signature can never double as
/// a signature over an arbitrary digest such as a transaction sighash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Challenge([u8; 32]);

/// A proof for one PSBT input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnershipProof {
    pub public_key: PublicKey,
    pub signature: Signature,
}

/// An ownership proof verification error.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("PSBT has no inputs")]
    NoInputs,
    #[error(
        "PSBT input map count mismatch: {transaction_inputs} transaction inputs and {input_maps} input maps"
    )]
    InputMapCountMismatch {
        transaction_inputs: usize,
        input_maps: usize,
    },
    #[error("proof count mismatch: {inputs} inputs and {proofs} proofs")]
    ProofCountMismatch { inputs: usize, proofs: usize },
    #[error("input {input_index} is missing its witness UTXO")]
    MissingWitnessUtxo { input_index: usize },
    #[error("input {input_index} does not spend a P2WPKH output")]
    NotP2wpkh { input_index: usize },
    #[error("proof public key does not match input {input_index}")]
    PublicKeyMismatch { input_index: usize },
    #[error("invalid signature for input {input_index}")]
    InvalidSignature { input_index: usize },
}

/// Computes the tagged hash of caller-provided context bytes.
///
/// The context must consist of fixed-length fields only (variable-length
/// fields make distinct sessions serialize to identical bytes) and must
/// include verifier-chosen session data, otherwise proofs are replayable
/// across sessions.
pub fn challenge(context: &[u8]) -> Challenge {
    let tag_hash = sha256::Hash::hash(CHALLENGE_TAG.as_bytes());
    let mut engine = sha256::Hash::engine();
    engine.input(tag_hash.as_byte_array());
    engine.input(tag_hash.as_byte_array());
    engine.input(context);
    Challenge(sha256::Hash::from_engine(engine).to_byte_array())
}

/// Signs a challenge with the key controlling one input.
pub fn sign(secret_key: &SecretKey, challenge: Challenge) -> OwnershipProof {
    let secp = Secp256k1::new();
    let message = Message::from_digest(challenge.0);

    OwnershipProof {
        public_key: PublicKey::from_secret_key(&secp, secret_key),
        signature: secp.sign_ecdsa(&message, secret_key),
    }
}

/// Verifies one proof for every input of an unsigned PSBT.
///
/// A passing result only proves control of the keys behind each input's
/// `witness_utxo` script — which the prover supplies inside the PSBT. It says
/// nothing about the chain: the caller must fetch every `previous_output` and
/// check that it exists unspent with exactly the claimed script and value,
/// otherwise the proof is vacuous.
pub fn verify_psbt(
    psbt: &bitcoin::psbt::Psbt,
    proofs: &[OwnershipProof],
    challenge: Challenge,
) -> Result<(), Error> {
    // The input maps are counted against the transaction inputs because serde
    // deserialization enforces no invariant between the two.
    let transaction_inputs = psbt.unsigned_tx.input.len();
    if transaction_inputs == 0 {
        return Err(Error::NoInputs);
    }
    if psbt.inputs.len() != transaction_inputs {
        return Err(Error::InputMapCountMismatch {
            transaction_inputs,
            input_maps: psbt.inputs.len(),
        });
    }
    if proofs.len() != transaction_inputs {
        return Err(Error::ProofCountMismatch {
            inputs: transaction_inputs,
            proofs: proofs.len(),
        });
    }

    for (input_index, (input, proof)) in psbt.inputs.iter().zip(proofs).enumerate() {
        let Some(witness_utxo) = &input.witness_utxo else {
            return Err(Error::MissingWitnessUtxo { input_index });
        };

        verify_input(proof, &witness_utxo.script_pubkey, challenge, input_index)?;
    }

    Ok(())
}

fn verify_input(
    proof: &OwnershipProof,
    script_pubkey: &bitcoin::Script,
    challenge: Challenge,
    input_index: usize,
) -> Result<(), Error> {
    if !script_pubkey.is_p2wpkh() {
        return Err(Error::NotP2wpkh { input_index });
    }

    let public_key_hash = hash160::Hash::hash(&proof.public_key.serialize());
    if script_pubkey.as_bytes()[2..] != public_key_hash[..] {
        return Err(Error::PublicKeyMismatch { input_index });
    }

    let secp = Secp256k1::verification_only();
    let message = Message::from_digest(challenge.0);
    secp.verify_ecdsa(&message, &proof.signature, &proof.public_key)
        .map_err(|_| Error::InvalidSignature { input_index })
}

#[cfg(test)]
mod tests {
    use bitcoin::absolute;
    use bitcoin::hashes::Hash;
    use bitcoin::transaction::Version;
    use bitcoin::{
        Amount, OutPoint, PubkeyHash, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Txid,
        WPubkeyHash, Witness,
    };

    use super::*;

    fn verify(
        proof: &OwnershipProof,
        script_pubkey: &bitcoin::Script,
        challenge: Challenge,
    ) -> Result<(), Error> {
        verify_input(proof, script_pubkey, challenge, 0)
    }

    #[test]
    fn round_trip() {
        let secret_key = secret_key(1);
        let proof = sign(&secret_key, challenge(b"round trip"));
        let script_pubkey = p2wpkh_script(&proof.public_key);

        assert!(verify(&proof, &script_pubkey, challenge(b"round trip")).is_ok());
    }

    #[test]
    fn wrong_challenge() {
        let secret_key = secret_key(1);
        let proof = sign(&secret_key, challenge(b"correct"));
        let script_pubkey = p2wpkh_script(&proof.public_key);

        assert!(matches!(
            verify(&proof, &script_pubkey, challenge(b"wrong")),
            Err(Error::InvalidSignature { input_index: 0 })
        ));
    }

    #[test]
    fn wrong_key() {
        let proof = sign(&secret_key(1), challenge(b"context"));
        let other_public_key = PublicKey::from_secret_key(&Secp256k1::new(), &secret_key(2));

        assert!(matches!(
            verify(
                &proof,
                &p2wpkh_script(&other_public_key),
                challenge(b"context")
            ),
            Err(Error::PublicKeyMismatch { input_index: 0 })
        ));
    }

    #[test]
    fn non_p2wpkh_script() {
        let proof = sign(&secret_key(1), challenge(b"context"));
        let pubkey_hash = PubkeyHash::hash(&proof.public_key.serialize());
        let script_pubkey = ScriptBuf::new_p2pkh(&pubkey_hash);

        assert!(matches!(
            verify(&proof, &script_pubkey, challenge(b"context")),
            Err(Error::NotP2wpkh { input_index: 0 })
        ));
    }

    #[test]
    fn verify_psbt_happy_path() {
        let challenge = challenge(b"psbt");
        let proofs = [
            sign(&secret_key(1), challenge),
            sign(&secret_key(2), challenge),
        ];
        let psbt = psbt([
            p2wpkh_script(&proofs[0].public_key),
            p2wpkh_script(&proofs[1].public_key),
        ]);

        assert!(verify_psbt(&psbt, &proofs, challenge).is_ok());
    }

    #[test]
    fn verify_psbt_proof_count_mismatch() {
        let challenge = challenge(b"psbt");
        let proof = sign(&secret_key(1), challenge);
        let psbt = psbt([
            p2wpkh_script(&proof.public_key),
            p2wpkh_script(&proof.public_key),
        ]);

        assert!(matches!(
            verify_psbt(&psbt, &[proof], challenge),
            Err(Error::ProofCountMismatch {
                inputs: 2,
                proofs: 1
            })
        ));
    }

    #[test]
    fn verify_psbt_missing_witness_utxo() {
        let challenge = challenge(b"psbt");
        let proofs = [
            sign(&secret_key(1), challenge),
            sign(&secret_key(2), challenge),
        ];
        let mut psbt = psbt([
            p2wpkh_script(&proofs[0].public_key),
            p2wpkh_script(&proofs[1].public_key),
        ]);
        psbt.inputs[1].witness_utxo = None;

        assert!(matches!(
            verify_psbt(&psbt, &proofs, challenge),
            Err(Error::MissingWitnessUtxo { input_index: 1 })
        ));
    }

    #[test]
    fn verify_psbt_rejects_swapped_proofs() {
        let challenge = challenge(b"psbt");
        let proofs = [
            sign(&secret_key(1), challenge),
            sign(&secret_key(2), challenge),
        ];
        let psbt = psbt([
            p2wpkh_script(&proofs[0].public_key),
            p2wpkh_script(&proofs[1].public_key),
        ]);

        assert!(matches!(
            verify_psbt(&psbt, &[proofs[1].clone(), proofs[0].clone()], challenge),
            Err(Error::PublicKeyMismatch { input_index: 0 })
        ));
    }

    #[test]
    fn challenge_is_deterministic_and_context_bound() {
        assert_eq!(challenge(b"context"), challenge(b"context"));
        assert_ne!(challenge(b"context"), challenge(b"other context"));
    }

    #[test]
    fn rejects_other_segwit_scripts() {
        let proof = sign(&secret_key(1), challenge(b"context"));
        let p2wsh = ScriptBuf::new_p2wsh(&bitcoin::WScriptHash::hash(b"script"));
        let p2tr_shaped = ScriptBuf::from_bytes([&[0x51, 0x20][..], &[0xAB; 32]].concat());
        let truncated_program = ScriptBuf::from_bytes([&[0x00, 0x14][..], &[0xAB; 10]].concat());

        for script_pubkey in [p2wsh, p2tr_shaped, truncated_program] {
            assert!(matches!(
                verify(&proof, &script_pubkey, challenge(b"context")),
                Err(Error::NotP2wpkh { input_index: 0 })
            ));
        }
    }

    #[test]
    fn rejects_high_s_signature() {
        let proof = sign(&secret_key(1), challenge(b"context"));
        let script_pubkey = p2wpkh_script(&proof.public_key);
        let malleated = OwnershipProof {
            public_key: proof.public_key,
            signature: high_s(&proof.signature),
        };

        assert!(matches!(
            verify(&malleated, &script_pubkey, challenge(b"context")),
            Err(Error::InvalidSignature { input_index: 0 })
        ));
    }

    #[test]
    fn proof_serde_round_trip() {
        let proof = sign(&secret_key(1), challenge(b"context"));
        let json = serde_json::to_string(&proof).expect("proof serializes");
        let deserialized: OwnershipProof = serde_json::from_str(&json).expect("proof deserializes");

        assert_eq!(proof, deserialized);
    }

    #[test]
    fn verify_psbt_rejects_zero_inputs() {
        let transaction = Transaction {
            version: Version::TWO,
            lock_time: absolute::LockTime::ZERO,
            input: vec![],
            output: vec![],
        };
        let psbt =
            bitcoin::psbt::Psbt::from_unsigned_tx(transaction).expect("transaction is unsigned");

        assert!(matches!(
            verify_psbt(&psbt, &[], challenge(b"psbt")),
            Err(Error::NoInputs)
        ));
    }

    #[test]
    fn verify_psbt_rejects_desynced_input_maps() {
        let challenge = challenge(b"psbt");
        let proof = sign(&secret_key(1), challenge);
        let mut psbt = psbt([
            p2wpkh_script(&proof.public_key),
            p2wpkh_script(&proof.public_key),
        ]);
        psbt.inputs.truncate(1);

        assert!(matches!(
            verify_psbt(&psbt, &[proof], challenge),
            Err(Error::InputMapCountMismatch {
                transaction_inputs: 2,
                input_maps: 1
            })
        ));
    }

    // n - s over big-endian bytes: the valid same-key signature with high S.
    fn high_s(signature: &Signature) -> Signature {
        let mut compact = signature.serialize_compact();
        let mut borrow = 0i16;
        for i in (0..32).rev() {
            let diff = bitcoin::secp256k1::constants::CURVE_ORDER[i] as i16
                - compact[32 + i] as i16
                - borrow;
            let (byte, next_borrow) = if diff < 0 { (diff + 256, 1) } else { (diff, 0) };
            compact[32 + i] = byte as u8;
            borrow = next_borrow;
        }
        Signature::from_compact(&compact).expect("malleated signature parses")
    }

    fn secret_key(byte: u8) -> SecretKey {
        SecretKey::from_slice(&[byte; 32]).expect("test secret key is valid")
    }

    fn p2wpkh_script(public_key: &PublicKey) -> ScriptBuf {
        ScriptBuf::new_p2wpkh(&WPubkeyHash::hash(&public_key.serialize()))
    }

    fn psbt(script_pubkeys: [ScriptBuf; 2]) -> bitcoin::psbt::Psbt {
        let inputs = [0, 1].map(|vout| TxIn {
            previous_output: OutPoint::new(Txid::all_zeros(), vout),
            script_sig: ScriptBuf::new(),
            sequence: Sequence::MAX,
            witness: Witness::new(),
        });
        let transaction = Transaction {
            version: Version::TWO,
            lock_time: absolute::LockTime::ZERO,
            input: inputs.to_vec(),
            output: vec![TxOut {
                value: Amount::from_sat(1_000),
                script_pubkey: ScriptBuf::new(),
            }],
        };
        let mut psbt = bitcoin::psbt::Psbt::from_unsigned_tx(transaction)
            .expect("test transaction is unsigned");

        for (input, script_pubkey) in psbt.inputs.iter_mut().zip(script_pubkeys) {
            input.witness_utxo = Some(TxOut {
                value: Amount::from_sat(1_000),
                script_pubkey,
            });
        }

        psbt
    }
}
