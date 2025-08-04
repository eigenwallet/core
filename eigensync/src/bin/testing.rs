use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
    ops::Deref,
    str::FromStr,
    time::Duration,
};

use anyhow::{Context, Result};
use automerge::{ActorId, AutoCommit, Change};
use autosurgeon::{hydrate, reconcile, Hydrate, Reconcile};
use eigensync::protocol::{client, Behaviour, BehaviourEvent, Request, Response, SerializedChange};
use libp2p::{
    futures::StreamExt,
    identity, noise, request_response,
    swarm::{self, SwarmEvent},
    tcp, yamux, Multiaddr, PeerId, Swarm, SwarmBuilder,
};
use uuid::Uuid;

pub struct Database {
    document: AutoCommit,
    state: State,
}

struct ServerDatabase {
    pub changes: Vec<Change>,
}

#[derive(Debug, Clone, Reconcile, Hydrate, PartialEq)]
pub struct State {
    swaps: HashMap<String, SwapState>,
}

#[derive(Debug, Clone, Reconcile, Hydrate, PartialEq, Hash, Eq)]
pub struct SwapState {
    #[key]
    pub state_id: Uuid,
    pub swap_id: u64,
    pub state: u64,
    pub amount: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut alice = Database::new();

    println!("1 alice changes: {}", alice.get_changes().len());

    alice
        .add_swap(SwapState {
            state_id: Uuid::new_v4(),
            swap_id: 0,
            state: 0,
            amount: 300,
        })
        .unwrap();

    println!("2 alice changes: {}", alice.get_changes().len());

    alice
        .add_swap(SwapState {
            state_id: Uuid::new_v4(),
            swap_id: 0,
            state: 1,
            amount: 300,
        })
        .unwrap();

    println!("3 alice changes: {}", alice.get_changes().len());

    let mut server = ServerDatabase::new();

    server.add_changes(alice.get_changes()).unwrap();

    println!("server changes: {}", server.changes.len());

    let mut bob = Database::new();

    bob.add_changes(server.changes.clone()).unwrap();

    println!("adding swap to bob");

    assert_eq!(alice.state, bob.state, "bob got alice swaps");

    println!("Client 2 state: {:?}", bob.state);

    bob.add_swap(SwapState {
        state_id: Uuid::new_v4(),
        swap_id: 1,
        state: 0,
        amount: 200,
    })
    .unwrap();

    server.add_changes(bob.get_changes()).unwrap();

    println!("server changes: {}", server.changes.len());

    let alice_changes = alice.get_changes();

    alice
        .add_changes(server.get_changes(alice_changes).clone())
        .unwrap();

    println!("alice changes: {}", alice.get_changes().len());

    assert_eq!(
        alice.state, bob.state,
        "Alice and Bob should have the same state"
    );

    Ok(())
}

impl Database {
    fn new() -> Self {
        let mut document = AutoCommit::new().with_actor(ActorId::random());

        let state = State {
            swaps: HashMap::new(),
        };

        reconcile(&mut document, &state)
            .context("Failed to reconcile")
            .unwrap();

        Self { document, state }
    }

    fn get_changes(&mut self) -> Vec<Change> {
        self.document
            .get_changes(&[])
            .iter()
            .map(|c| (*c).clone())
            .collect()
    }

    fn add_changes(&mut self, changes: Vec<Change>) -> anyhow::Result<()> {
        eprintln!("Number of changes to add: {}", changes.len());

        println!(
            "server doc changes before fork: {}",
            self.document.get_changes(&[]).len()
        );

        let mut server_doc = self.document.fork();

        println!(
            "server doc changes before apply: {}",
            server_doc.get_changes(&[]).len()
        );

        server_doc
            .apply_changes(changes)
            .context("Failed to apply changes")?;

        println!("server doc changes: {}", server_doc.get_changes(&[]).len());

        // Make sure server state is valid
        let _: State = hydrate(&server_doc).context("Couldn't hydrate doc into state")?;

        println!(
            "server doc changes after hydrate: {}",
            server_doc.get_changes(&[]).len()
        );

        self.document
            .merge(&mut server_doc)
            .context("Failed to merge")?;

        println!(
            "server doc changes after merge: {}",
            self.document.get_changes(&[]).len()
        );

        self.state = hydrate(&self.document).context("Couldn't hydrate doc into state")?;

        println!("state after add swap: {:?}", self.state);

        Ok(())
    }

    fn add_swap(&mut self, swap: SwapState) -> anyhow::Result<()> {
        self.state.swaps.insert(swap.state_id.to_string(), swap);

        reconcile(&mut self.document, self.state.clone()).context("Failed to reconcile")?;

        Ok(())
    }
}

impl ServerDatabase {
    fn new() -> Self {
        Self { changes: vec![] }
    }

    fn add_changes(&mut self, changes: Vec<Change>) -> anyhow::Result<()> {
        let mut new_changes = vec![];

        for change in changes {
            if !self.changes.contains(&change) {
                new_changes.push(change);
            }
        }

        self.changes.extend_from_slice(&new_changes);

        Ok(())
    }

    fn get_changes(&mut self, changes: Vec<Change>) -> Vec<Change> {
        let mut new_changes = vec![];

        for change in changes {
            if !self.changes.contains(&change) {
                new_changes.push(change);
            }
        }

        new_changes
    }
}
