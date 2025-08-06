// Copyright 2022 Hannes Furmans
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![warn(clippy::pedantic)]
#![deny(unsafe_code)]

use futures::future::BoxFuture;
use tor_interface::tor_provider::{self, CircuitToken, TcpOnionListener, TcpOrUnixOnionStream, TorProvider, OnionListener, OnionAddr};
use tor_interface::tor_crypto::{V3OnionServiceId, Ed25519PrivateKey, X25519PublicKey};
use libp2p::{
    core::transport::{ListenerId, TransportEvent},
    Multiaddr, Transport, TransportError,
};

use std::collections::{BTreeSet, HashMap};
use std::str::FromStr;
use tokio::net::TcpListener;

use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::net::SocketAddr;
use std::task::{Context, Poll};
use thiserror::Error;

mod address;
mod provider;

use address::{dangerous_extract, safe_extract};
pub use provider::OnionStreamStream;

use crate::AddressConversion;

pub use tor_interface;

/// Get a [`TorProvider`](`tor_provider::TorProvider`) from [`tor_interface`]
pub struct TorInterfaceTransport<T: TorProvider> {
    pub conversion_mode: AddressConversion,
    pub provider: Arc<Mutex<T>>,
    pub circuit: Option<CircuitToken>,

    /// Onion services we are listening on.
    listeners: HashMap<ListenerId, TcpListener>,

    /// Onion services we are running (implicitly excluded if ListenerId present)
    services: Vec<(TcpOnionListener, Option<ListenerId>)>,

    /// Services yet to be announced
    waiting_to_announce: HashMap<ListenerId, OnionAddr>,

    event_backlog: Vec<tor_provider::TorEvent>,

    /// Persistent list of services we already publish
    ///
    /// Tor delineates services by onion but libp2p does it by onion:port
    published_services: BTreeSet<V3OnionServiceId>,
}

#[derive(Debug, Error)]
pub enum TorInterfaceTransportError {
    #[error(transparent)]
    Client(#[from] tor_provider::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    match m.lock() {
        Ok(o) => o,
        Err(e) => e.into_inner(),
    }
}

fn bootstrap<T: TorProvider>(provider: &mut T) -> Result<(), tor_provider::Error> {
    match provider.bootstrap() {
        Err(tor_provider::Error::Generic(s)) if s.ends_with(" already bootstrapped") => Ok(()),
        res @ Ok(_) | res @ Err(_) => res,
    }
}

impl<T: TorProvider<Listener = TcpOnionListener>> TorInterfaceTransport<T> {
    /// Creates a new `TorClientBuilder`.
    pub fn from_provider(
        conversion_mode: AddressConversion,
        provider: Arc<Mutex<T>>,
        circuit: Option<CircuitToken>
    ) -> Result<Self, tor_provider::Error> {
        bootstrap(&mut *lock(&provider))?;
        Ok(Self {
            conversion_mode: conversion_mode,
            provider: provider,
            circuit: circuit,
            listeners: HashMap::new(),
            services: Vec::new(),
            waiting_to_announce: Default::default(),
            event_backlog: Default::default(),
            published_services: Default::default(),
        })
    }

    /// Call this function to instruct the transport to listen on a specific onion address
    /// You need to call this function **before** calling `listen_on`
    ///
    /// # Returns
    /// Returns the Multiaddr of the onion address that the transport can be instructed to listen on
    /// To actually listen on the address, you need to call [`listen_on()`] with the returned address
    ///
    /// # Blocks
    /// If listening fails with an `LegacyTorNotBootstrapped` error,
    /// `bootstrap()`s the provider and awaits bootstrap confirtmation
    ///
    /// # Errors
    /// Returns an error if we couldn't talk to the tor daemon
    pub fn add_onion_service(
        &mut self,
        private_key: &Ed25519PrivateKey,
        virt_port: u16,
        authorised_clients: Option<&[X25519PublicKey]>,
        socket_addr: Option<SocketAddr>,
    ) -> anyhow::Result<Multiaddr> {
        let ol = self.listener_or_bootstrap(|p| p.listener(private_key, virt_port, authorised_clients, socket_addr))?;
        ol.set_nonblocking(true)?;

        self.services.push((ol, None));

        let svid = V3OnionServiceId::from_private_key(&private_key);
        let multiaddr = svid.to_multiaddr(virt_port);

        Ok(multiaddr)
    }

    fn listener_or_bootstrap<R, F: FnMut(&mut T) -> Result<R, tor_provider::Error>>(&mut self, mut f: F) -> Result<R, tor_provider::Error> {
        loop {
            let attempt = f(&mut lock(&self.provider)); // Moving this into the match clause deadlocks (Guard still borrowed)
            match attempt {
                Err(tor_provider::Error::Generic(s)) if s.ends_with(" not bootstrapped") => {
                    bootstrap(&mut *lock(&self.provider))?;
                    self.event_backlog.extend(lock(&self.provider).update()?);
                }
                res @ Ok(_) | res @ Err(_) => return res,
            }
        }
    }
}

trait HsIdExt {
    fn to_multiaddr(&self, port: u16) -> Multiaddr;
}

impl HsIdExt for V3OnionServiceId {
    /// Convert an `V3OnionServiceId` to a `Multiaddr`
    fn to_multiaddr(&self, port: u16) -> Multiaddr {
        // The internal representation of V3OnionServiceId is 52 characters, so we can't re-use it here.
        let multiaddress_string = format!("/onion3/{self}:{port}");

        Multiaddr::from_str(&multiaddress_string)
            .expect("A valid onion address to be convertible to a Multiaddr")
    }
}

trait OnionAddrExt {
    fn to_multiaddr(&self) -> Multiaddr;
}

impl OnionAddrExt for OnionAddr {
    fn to_multiaddr(&self) -> Multiaddr {
        let OnionAddr::V3(v3) = self;
        v3.service_id().to_multiaddr(v3.virt_port())
    }
}

#[cfg(test)]
#[test]
fn to_multiaddr() {
    use tor_interface::tor_crypto::Ed25519PublicKey;
    use libp2p::multiaddr::multiaddr;
    let test = V3OnionServiceId::from_public_key(&Ed25519PublicKey::from_raw(&[0; 32]).unwrap()).to_multiaddr(12345);
    assert_eq!(
        test,
        multiaddr!(Onion3((
            [
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0xCD, 0x0E, 0x03
            ],
            12345
        )))
    );
    assert_eq!(
        test.to_string(),
        "/onion3/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaam2dqd:12345"
    );
}


impl<T: TorProvider<Stream = TcpOrUnixOnionStream> + Send + Sync + 'static> Transport for TorInterfaceTransport<T> {
    type Error = TorInterfaceTransportError;
    type Output = OnionStreamStream;
    type ListenerUpgrade = std::future::Ready<Result<Self::Output, TorInterfaceTransportError>>;
    type Dial = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn listen_on(
        &mut self,
        id: ListenerId,
        onion_address: Multiaddr,
    ) -> Result<(), TransportError<Self::Error>> {
        // If the address is not an onion3 address, return an error
        if !matches!(onion_address.into_iter().nth(0), Some(libp2p::multiaddr::Protocol::Onion3(_))) {
            return Err(TransportError::MultiaddrNotSupported(onion_address));
        }

        // Find the running onion service that matches the requested address
        // If we find it, tag it in [`services`] and insert it into [`listeners`]
        let service = self
            .services
            .iter_mut()
            .find(|(service, listener_id)| listener_id.is_none() && service.address().to_multiaddr() == onion_address);
        let Some((service, listener_id)) = service
        else {
            return Err(TransportError::MultiaddrNotSupported(onion_address));
        };


        let listener = service.try_clone_inner().and_then(TcpListener::from_std).map_err(TorInterfaceTransportError::Io).map_err(TransportError::Other)?;
        *listener_id = Some(id);

        self.listeners.insert(id, listener);
        self.waiting_to_announce.insert(id, service.address().clone());

        Ok(())
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        // Take the listener out of the map. This will stop listening on onion service for libp2p connections (we will not poll it anymore)
        // However, we will not stop the onion service itself because we might want to reuse it later
        // The onion service will be stopped when the transport is dropped
        if let Some(_) = self.listeners.remove(&id) {
            let Some((_, listener_id)) = self.services.iter_mut().find(|(_, listener_id)| *listener_id == Some(id))
                else { unreachable!() };
            *listener_id = None;
            self.waiting_to_announce.remove(&id);
            return true;
        }

        false
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let maybe_tor_addr = match self.conversion_mode {
            AddressConversion::DnsOnly => safe_extract(&addr),
            AddressConversion::IpAndDns => dangerous_extract(&addr),
        };

        let Some(tor_address) = maybe_tor_addr
            else { return Err(TransportError::MultiaddrNotSupported(addr)); };
        let provider = self.provider.clone();
        let circuit = self.circuit;

        Ok(Box::pin(async move {
            let stream = lock(&provider).connect(tor_address, circuit).map_err(Self::Error::Client)?;

            tracing::debug!(%addr, "Established connection to peer through Tor");

            OnionStreamStream::from_onion_stream(stream).map_err(Self::Error::Io)
        }))
    }

    fn dial_as_listener(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        self.dial(addr)
    }

    fn address_translation(&self, _: &Multiaddr, _: &Multiaddr) -> Option<Multiaddr> {
        None
    }

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        while !&self.event_backlog.is_empty() {
            match self.event_backlog.swap_remove(0) {
                tor_provider::TorEvent::BootstrapStatus { .. } => {}
                tor_provider::TorEvent::BootstrapComplete => tracing::debug!("Tor bootstrap complete"),
                tor_provider::TorEvent::LogReceived { line } => tracing::debug!(%line),
                tor_provider::TorEvent::OnionServicePublished { service_id } => { self.published_services.insert(service_id); },
            }
        }

        // This is HashMap::extract_if() but that's unstable rn; not perf-sensitive (self.waiting_to_announce.len() is almost always 0)
        if let Some(listener_id) = self.waiting_to_announce.iter().find(|(_, addr)| {
            let OnionAddr::V3(addr) = addr;
            self.published_services.contains(addr.service_id())
        }).map(|(listener_id, _)| listener_id).copied() {
            return Poll::Ready(TransportEvent::NewAddress {
                listener_id,
                listen_addr: self.waiting_to_announce.remove(&listener_id).unwrap(/*key from find()*/).to_multiaddr(),
            });
        }

        let new_events = lock(&self.provider).update().unwrap_or(vec![]);
        self.event_backlog.extend(new_events);
        if !self.event_backlog.is_empty() {
            return self.poll(cx);
        }

        for (&listener_id, listener) in &mut self.listeners {
            match listener.poll_accept(cx) {
                Poll::Ready(Ok((caller, _))) => {
                    let service_addr = self.services.iter().find(|(_, li)| *li == Some(listener_id)).map(|(ol, _)| ol.address());
                    let multi = service_addr.map(|ra| ra.to_multiaddr()).unwrap_or(Multiaddr::empty());

                    return Poll::Ready(TransportEvent::Incoming {
                        listener_id,
                        upgrade: std::future::ready(Ok((caller, service_addr.cloned()).into())),
                        local_addr: multi.clone(),
                        send_back_addr: multi,
                    });
                }

                Poll::Ready(Err(err)) => {
                    return Poll::Ready(TransportEvent::ListenerError { listener_id, error: err.into() });
                }

                Poll::Pending => {},
            }
        }

        Poll::Pending
    }
}
