//! A behaviour that emits a Event to the Swarm when it notices that a specific peer supports a specific protocol.

// emits something like { SupportsProtocol(protocol: StreamProtocol, peer: PeerId) }
// uses its connectionhandler to listen for ConnectionEvent::RemoteProtocolsChange
// constructor takes a single StreamProtocol

use std::collections::VecDeque;

use libp2p::{
    core::upgrade,
    swarm::{handler::ProtocolsChange, ConnectionHandler, NetworkBehaviour, SubstreamProtocol},
    PeerId, StreamProtocol,
};

pub struct Behaviour {
    interesting_protocol: StreamProtocol,
    to_swarm: VecDeque<Event>,
}

impl Behaviour {
    pub fn new(interesting_protocol: StreamProtocol) -> Self {
        Self {
            interesting_protocol,
            to_swarm: VecDeque::new(),
        }
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = NoticeProtocolSupportConnectionHandler;

    type ToSwarm = Event;

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: libp2p::swarm::ConnectionId,
        peer: libp2p::PeerId,
        local_addr: &libp2p::Multiaddr,
        remote_addr: &libp2p::Multiaddr,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        Ok(NoticeProtocolSupportConnectionHandler::new(
            self.interesting_protocol.clone(),
        ))
    }

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: libp2p::swarm::ConnectionId,
        peer: libp2p::PeerId,
        addr: &libp2p::Multiaddr,
        role_override: libp2p::core::Endpoint,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        Ok(NoticeProtocolSupportConnectionHandler::new(
            self.interesting_protocol.clone(),
        ))
    }

    fn on_swarm_event(&mut self, event: libp2p::swarm::FromSwarm) {
        // nothing to do here
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: libp2p::PeerId,
        _connection_id: libp2p::swarm::ConnectionId,
        event: libp2p::swarm::THandlerOutEvent<Self>,
    ) {
        self.to_swarm
            .push_back(Event::SupportsProtocol { peer: peer_id });
    }

    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<libp2p::swarm::ToSwarm<Self::ToSwarm, libp2p::swarm::THandlerInEvent<Self>>>
    {
        if let Some(event) = self.to_swarm.pop_front() {
            return std::task::Poll::Ready(libp2p::swarm::ToSwarm::GenerateEvent(event));
        }

        std::task::Poll::Pending
    }
}

pub struct NoticeProtocolSupportConnectionHandler {
    interesting_protocol: StreamProtocol,
    to_behaviour: VecDeque<ToBehaviour>,
}

impl NoticeProtocolSupportConnectionHandler {
    fn new(interesting_protocol: StreamProtocol) -> Self {
        Self {
            interesting_protocol,
            to_behaviour: VecDeque::new(),
        }
    }
}

#[derive(Debug)]
pub enum Event {
    SupportsProtocol { peer: PeerId },
}

#[derive(Debug)]
pub enum ToBehaviour {
    SupportsProtocol,
}

impl ConnectionHandler for NoticeProtocolSupportConnectionHandler {
    type FromBehaviour = ();
    type ToBehaviour = ToBehaviour;

    type InboundProtocol = upgrade::DeniedUpgrade;
    type OutboundProtocol = upgrade::DeniedUpgrade;

    type InboundOpenInfo = ();
    type OutboundOpenInfo = ();

    fn listen_protocol(
        &self,
    ) -> libp2p::swarm::SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(upgrade::DeniedUpgrade, ())
    }

    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<
        libp2p::swarm::ConnectionHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            Self::ToBehaviour,
        >,
    > {
        if let Some(to_behaviour) = self.to_behaviour.pop_front() {
            return std::task::Poll::Ready(libp2p::swarm::ConnectionHandlerEvent::NotifyBehaviour(
                to_behaviour,
            ));
        }

        std::task::Poll::Pending
    }

    fn on_behaviour_event(&mut self, _event: Self::FromBehaviour) {
        unreachable!("This connection handler should not receive events");
    }

    fn on_connection_event(
        &mut self,
        event: libp2p::swarm::handler::ConnectionEvent<
            Self::InboundProtocol,
            Self::OutboundProtocol,
            Self::InboundOpenInfo,
            Self::OutboundOpenInfo,
        >,
    ) {
        if let libp2p::swarm::handler::ConnectionEvent::RemoteProtocolsChange(protocols) = event {
            if let ProtocolsChange::Added(protocols) = protocols {
                for protocol in protocols {
                    if protocol == &self.interesting_protocol {
                        self.to_behaviour.push_back(ToBehaviour::SupportsProtocol);
                    }
                }
            }
        }
    }
}
