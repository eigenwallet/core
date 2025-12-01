//! A network behaviour that observes our connections to peers and emits events which can
//! then be used to be (among other things) emitted to the UI to display which peers we are connected to.
use libp2p::{
    swarm::{NetworkBehaviour, ToSwarm},
    Multiaddr, PeerId,
};
use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, task::Poll};
use typeshare::typeshare;

use crate::behaviour_util::{AddressTracker, ConnectionTracker};

pub struct Behaviour {
    connections: ConnectionTracker,
    addresses: AddressTracker,

    /// Queue of events to be sent to the swarm
    to_swarm: VecDeque<Event>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[typeshare]
pub struct Event {
    #[typeshare(serialized_as = "string")]
    pub peer_id: PeerId,
    pub update: ConnectionChange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[typeshare]
#[serde(tag = "type", content = "content")]
pub enum ConnectionChange {
    /// Emitted when the connection status of a peer changes
    Connection(ConnectionStatus),
    /// Emitted when the address changes that we display to the user
    LastAddress(#[typeshare(serialized_as = "string")] Multiaddr),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[typeshare]
pub enum ConnectionStatus {
    Connected,
    Disconnected,
    Dialing,
}

impl Behaviour {
    pub fn new() -> Self {
        Self {
            connections: ConnectionTracker::new(),
            addresses: AddressTracker::new(),
            to_swarm: VecDeque::new(),
        }
    }

    // TODO: We could extract this into the trackers. They could have an internal queue which we can pop in our poll function.
    fn emit_connection_status(&mut self, peer_id: PeerId) {
        let status = if self.connections.has_inflight_dial(&peer_id) {
            ConnectionStatus::Dialing
        } else if self.connections.is_connected(&peer_id) {
            ConnectionStatus::Connected
        } else {
            ConnectionStatus::Disconnected
        };

        self.to_swarm.push_back(Event {
            peer_id,
            update: ConnectionChange::Connection(status),
        });
    }

    fn emit_last_seen_address(&mut self, peer_id: PeerId) {
        let Some(address) = self.addresses.last_seen_address(&peer_id) else {
            return;
        };

        self.to_swarm.push_back(Event {
            peer_id,
            update: ConnectionChange::LastAddress(address),
        });
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = libp2p::swarm::dummy::ConnectionHandler;
    type ToSwarm = Event;

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: libp2p::swarm::ConnectionId,
        _peer: libp2p::PeerId,
        _local_addr: &libp2p::Multiaddr,
        _remote_addr: &libp2p::Multiaddr,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        Ok(libp2p::swarm::dummy::ConnectionHandler)
    }

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: libp2p::swarm::ConnectionId,
        _peer: libp2p::PeerId,
        _addr: &libp2p::Multiaddr,
        _role_override: libp2p::core::Endpoint,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        Ok(libp2p::swarm::dummy::ConnectionHandler)
    }

    fn on_swarm_event(&mut self, event: libp2p::swarm::FromSwarm) {
        if let Some(peer_id) = self.connections.handle_swarm_event(event) {
            self.emit_connection_status(peer_id);
        }

        if let Some(peer_id) = self.addresses.handle_swarm_event(event) {
            self.emit_last_seen_address(peer_id);
        }
    }

    fn on_connection_handler_event(
        &mut self,
        _peer_id: libp2p::PeerId,
        _connection_id: libp2p::swarm::ConnectionId,
        _event: libp2p::swarm::THandlerOutEvent<Self>,
    ) {
        unreachable!("No event will be produced by a dummy handler.");
    }

    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, libp2p::swarm::THandlerInEvent<Self>>> {
        if let Some(event) = self.to_swarm.pop_front() {
            return Poll::Ready(ToSwarm::GenerateEvent(event));
        }

        Poll::Pending
    }

    fn handle_pending_inbound_connection(
        &mut self,
        _connection_id: libp2p::swarm::ConnectionId,
        _local_addr: &Multiaddr,
        _remote_addr: &Multiaddr,
    ) -> Result<(), libp2p::swarm::ConnectionDenied> {
        Ok(())
    }

    fn handle_pending_outbound_connection(
        &mut self,
        connection_id: libp2p::swarm::ConnectionId,
        maybe_peer: Option<PeerId>,
        _addresses: &[Multiaddr],
        _effective_role: libp2p::core::Endpoint,
    ) -> Result<Vec<Multiaddr>, libp2p::swarm::ConnectionDenied> {
        if let Some(peer_id) = self
            .connections
            .handle_pending_outbound_connection(connection_id, maybe_peer)
        {
            self.emit_connection_status(peer_id);
        }

        Ok(std::vec![])
    }
}
