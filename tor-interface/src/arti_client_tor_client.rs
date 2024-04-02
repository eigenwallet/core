// standard
use std::io::ErrorKind;

// internal crates
use crate::tor_crypto::*;
use crate::tor_provider;
use crate::tor_provider::*;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("not implemented")]
    NotImplemented(),
}

impl From<Error> for crate::tor_provider::Error {
    fn from(error: Error) -> Self {
        crate::tor_provider::Error::Generic(error.to_string())
    }
}

pub struct ArtiClientOnionListener {
}

impl OnionListenerImpl for ArtiClientOnionListener {
    fn set_nonblocking(&self, _nonblocking: bool) -> Result<(), std::io::Error> {
        Err(std::io::Error::new(ErrorKind::Other, "not implemented"))
    }
    fn accept(&self) -> Result<Option<OnionStream>, std::io::Error> {
        Err(std::io::Error::new(ErrorKind::Other, "not implemented"))
    }
}

#[derive(Default)]
pub struct ArtiClientTorClient {
}

impl ArtiClientTorClient {

}

impl TorProvider for ArtiClientTorClient {
    fn update(&mut self) -> Result<Vec<TorEvent>, tor_provider::Error> {
        Err(Error::NotImplemented().into())
    }

    fn bootstrap(&mut self) -> Result<(), tor_provider::Error> {
        Err(Error::NotImplemented().into())
    }

    fn add_client_auth(
        &mut self,
        _service_id: &V3OnionServiceId,
        _client_auth: &X25519PrivateKey,
    ) -> Result<(), tor_provider::Error> {
        Err(Error::NotImplemented().into())
    }

    fn remove_client_auth(
        &mut self,
        _service_id: &V3OnionServiceId,
    ) -> Result<(), tor_provider::Error> {
        Err(Error::NotImplemented().into())
    }

    fn connect(
        &mut self,
        _service_id: &V3OnionServiceId,
        _virt_port: u16,
        _circuit: Option<CircuitToken>,
    ) -> Result<OnionStream, tor_provider::Error> {
        Err(Error::NotImplemented().into())
    }

    fn listener(
        &mut self,
        _private_key: &Ed25519PrivateKey,
        _virt_port: u16,
        _authorized_clients: Option<&[X25519PublicKey]>,
    ) -> Result<OnionListener, tor_provider::Error> {
        Err(Error::NotImplemented().into())
    }

    fn generate_token(&mut self) -> CircuitToken {
        0usize
    }

    fn release_token(&mut self, _token: CircuitToken) {

    }
}
