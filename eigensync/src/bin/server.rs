use std::{collections::HashMap, str::FromStr, time::Duration};

use anyhow::{Context, Result};
use automerge::Change;
use eigensync::{
    protocol::{BehaviourEvent, SerializedChange},
    server, Request, Response,
};
use libp2p::{
    futures::StreamExt, identity, noise, request_response, swarm::SwarmEvent, tcp, yamux,
    Multiaddr, PeerId, SwarmBuilder,
};

struct Database {
    pub changes: HashMap<PeerId, Vec<Change>>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut db = Database::new();

    let mut swarm = SwarmBuilder::with_existing_identity(identity::Keypair::generate_ed25519())
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )
        .context("Failed to create TCP transport")?
        .with_behaviour(|_| Ok(server()))
        .context("Failed to create behaviour")?
        .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(Duration::MAX))
        .build();

    swarm.listen_on(Multiaddr::from_str("/ip4/127.0.0.1/tcp/3333")?)?;

    println!(
        "Listening on /ip4/127.0.0.1/tcp/3333/p2p/{}",
        swarm.local_peer_id()
    );

    loop {
        tokio::select! {
            event = swarm.select_next_some() => {
                match event {
                    SwarmEvent::Behaviour(BehaviourEvent::Sync(request_response::Event::Message {
                        peer,
                        message,
                    })) => {
                        match message {
                            request_response::Message::Request { request, channel, request_id } => {
                                eprintln!("Received request of id {:?}", request_id);

                                match request {
                                    Request::GetChanges { current_changes } => {
                                        eprintln!("Received GetChanges request");
                                        let changes = db.get_changes(peer, current_changes);
                                        swarm.behaviour_mut().send_response(channel, Response::NewChanges { changes }).expect("Failed to send response");
                                        eprintln!("DB: {:?}", db.changes);
                                    }
                                    Request::AddChanges { changes } => {
                                        eprintln!("Received AddChanges request");
                                        db.add_changes(peer, changes.into_iter().map(|c| c.into()).collect());
                                        swarm.behaviour_mut().send_response(channel, Response::ChangesAdded).expect("Failed to send response");
                                        eprintln!("DB: {:?}", db.changes);
                                    }
                                }
                            }
                            request_response::Message::Response { request_id, .. } => eprintln!("Received response for request of id {:?}", request_id),
                        }
                    }
                    other => eprintln!("Received event: {:?}", other),
                }
            }
        }
    }
}

impl Database {
    fn new() -> Self {
        Self {
            changes: HashMap::new(),
        }
    }

    fn get_changes(
        &self,
        peer_id: PeerId,
        current_changes: Vec<SerializedChange>,
    ) -> Vec<SerializedChange> {
        let stored_changes = self
            .changes
            .get(&peer_id)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|c| c.into())
            .filter(|c| !current_changes.contains(&c))
            .collect();

        stored_changes
    }

    fn add_changes(&mut self, peer_id: PeerId, changes: Vec<Change>) {
        self.changes.entry(peer_id).or_default().extend(changes);
    }
}
