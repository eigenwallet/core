// standard

// internal crates
use crate::tor_crypto::*;
use crate::tor_provider::*;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Function not implemented")]
    NotImplemented(),
}

pub struct MockCircuitToken {}
impl CircuitToken for MockCircuitToken {}

pub struct MockOnionListener {}

impl OnionListener for MockOnionListener {
    fn set_nonblocking(&self, nonblocking: bool) -> Result<(), std::io::Error> {
        Ok(())
    }
    fn accept(&self) -> Result<Option<OnionStream>, std::io::Error> {
        Ok(None)
    }
}

pub struct MockTorClient {}

impl TorProvider<MockCircuitToken, MockOnionListener> for MockTorClient {
    type Error = Error;

    fn update(&mut self) -> Result<Vec<TorEvent>, Self::Error> {
        Err(Error::NotImplemented())
    }

    fn bootstrap(&mut self) -> Result<(), Self::Error> {
        Err(Error::NotImplemented())
    }

    fn add_client_auth(
        &mut self,
        service_id: &V3OnionServiceId,
        clien_auth: &X25519PrivateKey,
    ) -> Result<(), Self::Error> {
        Err(Error::NotImplemented())
    }

    fn remove_client_auth(&mut self, service_id: &V3OnionServiceId) -> Result<(), Self::Error> {
        Err(Error::NotImplemented())
    }

    fn connect(
        &mut self,
        service_id: &V3OnionServiceId,
        virt_port: u16,
        circuit: Option<MockCircuitToken>,
    ) -> Result<OnionStream, Self::Error> {
        Err(Error::NotImplemented())
    }

    fn listener(
        &mut self,
        private_key: &Ed25519PrivateKey,
        virt_port: u16,
        authorized_clients: Option<&[X25519PublicKey]>,
    ) -> Result<MockOnionListener, Self::Error> {
        Err(Error::NotImplemented())
    }
}
