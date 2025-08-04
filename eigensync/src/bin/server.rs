use std::{collections::HashMap, str::FromStr, time::Duration};

use anyhow::{Context, Result};
use automerge::Change;
use eigensync::protocol::{server, BehaviourEvent, Request, Response, SerializedChange};
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

    // for constant peer id
    let keypair = identity::Keypair::ed25519_from_bytes(
        hex::decode("6c0f291615972e0cc7efa86dc19480ba9999f64b79eee98cebdfdfb1fbf1dea6").unwrap(),
    )
    .unwrap();

    let mut swarm = SwarmBuilder::with_existing_identity(keypair)
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
                            request_response::Message::Request { request, channel, .. } => {
                                eprintln!("Received request from client {:?}", peer);

                                match request {
                                    Request::GetChanges { changes } => {
                                        eprintln!("Received GetChanges request");
                                        let changes = db.get_changes(peer, changes);
                                        eprintln!("Got {} new changes for client", changes.len());
                                        swarm.behaviour_mut().send_response(channel, Response::NewChanges { changes }).expect("Failed to send response");
                                        eprintln!("DB size: {:?}", db.changes[&peer].len());
                                    }
                                    Request::AddChanges { changes } => {
                                        eprintln!("Received AddChanges request");
                                        db.add_changes(peer, changes.into_iter().map(|c| c.into()).collect());
                                        swarm.behaviour_mut().send_response(channel, Response::ChangesAdded).expect("Failed to send response");
                                        eprintln!("DB size: {:?}", db.changes[&peer].len());
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
        let Some(stored_changes) = self.changes.get(&peer_id) else {
            eprintln!("No changes stored for client");
            return vec![];
        };

        stored_changes
            .clone()
            .into_iter()
            .filter_map(|change| {
                let serialized = change.into();

                if current_changes.contains(&serialized) {
                    eprintln!(
                        "Skipping change because client already has it: {:?}",
                        serialized
                    );
                    None
                } else {
                    eprintln!("Adding change to client");
                    Some(serialized)
                }
            })
            .collect()
    }

    fn add_changes(&mut self, peer_id: PeerId, changes: Vec<Change>) {
        if !self.changes.contains_key(&peer_id) {
            eprintln!("No changes stored for client, creating new entry");
            self.changes.insert(peer_id, vec![]);
        }

        eprintln!("Adding {} changes to client", changes.len());
        self.changes.get_mut(&peer_id).unwrap().extend(changes);
    }
}
