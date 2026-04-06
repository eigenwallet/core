use std::collections::HashMap;
use std::sync::Arc;
use std::task::{Context, Poll};

use libp2p::request_response;
use libp2p::swarm::{
    ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, THandler, THandlerInEvent,
    THandlerOutEvent, ToSwarm,
};
use libp2p::{Multiaddr, PeerId};
use swap_p2p::protocols::wormhole as proto;

use super::WormholeStore;

pub struct Behaviour {
    inner: proto::InnerBehaviour,
    store: Arc<dyn WormholeStore + Send + Sync>,
    /// Cached wormhole addresses per peer: (address, active).
    wormholes: HashMap<PeerId, (Multiaddr, bool)>,
}

impl Behaviour {
    pub fn new(store: Arc<dyn WormholeStore + Send + Sync>) -> Self {
        Self {
            inner: proto::bob(),
            store,
            wormholes: HashMap::new(),
        }
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = <proto::InnerBehaviour as NetworkBehaviour>::ConnectionHandler;
    type ToSwarm = void::Void;

    fn handle_established_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer_id: PeerId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.inner.handle_established_inbound_connection(
            connection_id,
            peer_id,
            local_addr,
            remote_addr,
        )
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer_id: PeerId,
        addr: &Multiaddr,
        role_override: libp2p::core::Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.inner.handle_established_outbound_connection(
            connection_id,
            peer_id,
            addr,
            role_override,
        )
    }

    fn handle_pending_outbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        maybe_peer: Option<PeerId>,
        _addresses: &[Multiaddr],
        _effective_role: libp2p::core::Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        let Some(peer_id) = maybe_peer else {
            return Ok(vec![]);
        };
        let Some((addr, _active)) = self.wormholes.get(&peer_id) else {
            return Ok(vec![]);
        };
        // Always suggest the wormhole address.
        // Ordering (front vs back) is handled by the dialer — we just provide it.
        Ok(vec![addr.clone()])
    }

    fn on_swarm_event(&mut self, event: FromSwarm<'_>) {
        self.inner.on_swarm_event(event);
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        self.inner
            .on_connection_handler_event(peer_id, connection_id, event);
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        while let Poll::Ready(event) = self.inner.poll(cx) {
            match event {
                ToSwarm::GenerateEvent(event) => {
                    if let request_response::Event::Message {
                        peer,
                        message:
                            request_response::Message::Request {
                                request, channel, ..
                            },
                        ..
                    } = event
                    {
                        tracing::debug!(
                            %peer,
                            address = %request.address,
                            active = request.active,
                            "Received wormhole from peer"
                        );

                        self.wormholes
                            .insert(peer, (request.address.clone(), request.active));

                        // Persist asynchronously — fire and forget
                        let store = Arc::clone(&self.store);
                        let address = request.address;
                        let active = request.active;
                        tokio::spawn(async move {
                            if let Err(e) = store.store_wormhole(peer, address, active).await {
                                tracing::warn!(%peer, error = ?e, "Failed to persist wormhole");
                            }
                        });

                        // Ack the request
                        let _ = self.inner.send_response(channel, ());
                    }
                }
                other => {
                    return Poll::Ready(other.map_out(|_| unreachable!()));
                }
            }
        }

        Poll::Pending
    }
}
