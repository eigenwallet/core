use curve25519_dalek::Scalar as DalekScalar;
use curve25519_dalek_ng::scalar::Scalar as DalekNgScalar;

pub trait IntoDalek {
    type Target;
    fn into_dalek(self) -> Self::Target;
}

pub trait IntoDalekNg {
    type Target;
    fn into_dalek_ng(self) -> Self::Target;
}

impl IntoDalek for DalekNgScalar {
    type Target = DalekScalar;

    fn into_dalek(self) -> Self::Target {
        DalekScalar::from_bytes_mod_order(self.to_bytes())
    }
}

impl IntoDalekNg for DalekScalar {
    type Target = DalekNgScalar;

    fn into_dalek_ng(self) -> Self::Target {
        DalekNgScalar::from_bytes_mod_order(self.to_bytes())
    }
}
