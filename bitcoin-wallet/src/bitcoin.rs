// Re-export transaction types
pub use crate::cancel::{CancelTimelock, PunishTimelock, TxCancel};
pub use crate::early_refund::TxEarlyRefund;
pub use crate::lock::TxLock;
pub use crate::punish::TxPunish;
pub use crate::redeem::TxRedeem;
pub use crate::refund::TxRefund;
pub use crate::timelocks::{BlockHeight, ExpiredTimelocks};

// Re-export bitcoin types
pub use ::bitcoin::amount::Amount;
pub use ::bitcoin::psbt::Psbt as PartiallySignedTransaction;
pub use ::bitcoin::{Address, AddressType, Network, Transaction, Txid};

// Re-export crypto types
pub use ecdsa_fun::adaptor::EncryptedSignature;
pub use ecdsa_fun::fun::Scalar;
pub use ecdsa_fun::Signature;

// Re-export wallet
pub use crate::wallet::{Wallet, ScriptStatus};

#[cfg(test)]
pub use crate::wallet::TestWalletBuilder;

// Re-export from ext module
pub use crate::ext::{
    SecretKey, PublicKey, verify_sig, verify_encsig, build_shared_output_descriptor,
    recover, current_epoch, extract_ecdsa_sig, InvalidSignature, InvalidEncryptedSignature,
    NoInputs, TooManyInputs, EmptyWitnessStack, NotThreeWitnesses, RpcErrorCode, 
    parse_rpc_error_code, bitcoin_address
};