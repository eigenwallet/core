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

pub trait IntoMoneroOxide {
    type Target;
    fn into_monero_oxide(self) -> Self::Target;
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

impl IntoDalek for monero_oxide_wallet::ed25519::Scalar {
    type Target = DalekScalar;

    fn into_dalek(self) -> Self::Target {
        DalekScalar::from_bytes_mod_order(
            monero_oxide_ext::PrivateKey::from_scalar(self).to_bytes(),
        )
    }
}

impl IntoDalekNg for monero_oxide_wallet::ed25519::Scalar {
    type Target = DalekNgScalar;

    fn into_dalek_ng(self) -> Self::Target {
        DalekNgScalar::from_bytes_mod_order(
            monero_oxide_ext::PrivateKey::from_scalar(self).to_bytes(),
        )
    }
}

impl IntoMoneroOxide for DalekScalar {
    type Target = monero_oxide_wallet::ed25519::Scalar;

    fn into_monero_oxide(self) -> Self::Target {
        monero_oxide_wallet::ed25519::Scalar::read(&mut &self.to_bytes()[..]).unwrap()
    }
}

impl IntoMoneroOxide for DalekNgScalar {
    type Target = monero_oxide_wallet::ed25519::Scalar;

    fn into_monero_oxide(self) -> Self::Target {
        monero_oxide_wallet::ed25519::Scalar::read(&mut &self.to_bytes()[..]).unwrap()
    }
}
