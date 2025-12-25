use crate::bitcoin::Scalar;
use ecdsa_fun::fun::marker::{NonZero, Secret};

pub trait ScalarExt {
    fn to_secpfun_scalar(&self) -> ecdsa_fun::fun::Scalar;
}

impl ScalarExt for monero_oxide_wallet::ed25519::Scalar {
    fn to_secpfun_scalar(&self) -> Scalar<Secret, NonZero> {
        let mut little_endian_bytes = [0u8; 32];
        self.write(&mut &mut little_endian_bytes[..])
            .expect("writing 32 into 32");

        little_endian_bytes.reverse();
        let big_endian_bytes = little_endian_bytes;

        ecdsa_fun::fun::Scalar::from_bytes(big_endian_bytes)
            .expect("valid scalar")
            .non_zero()
            .expect("non-zero scalar")
    }
}
