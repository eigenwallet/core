use std::{
    collections::{HashMap, HashSet}, hash::Hash, marker::PhantomData, ops::Deref, str::FromStr, sync::{Arc, Mutex, OnceLock}, time::Duration
};

use anyhow::{Context, Result};
use automerge::{ActorId, AutoCommit, Change};
use autosurgeon::{hydrate, reconcile, Hydrate, Reconcile};
use eigensync::{protocol::{client, Behaviour, BehaviourEvent, ChannelRequest, Response, SerializedChange}, EigensyncHandle, ServerDatabase};
use libp2p::{
    futures::StreamExt,
    identity, noise, request_response,
    swarm::{self, SwarmEvent},
    tcp, yamux, Multiaddr, PeerId, Swarm, SwarmBuilder,
};
use uuid::Uuid;

#[derive(Debug, Clone, Reconcile, Hydrate, PartialEq, Default)]
pub struct State {
    swaps: HashMap<String, SwapState>,
}

#[derive(Debug, Clone, Reconcile, Hydrate, PartialEq, Hash, Eq, Default)]
pub struct SwapState {
    #[key]
    pub state_id: Uuid,
    pub swap_id: u64,
    pub state: u64,
    pub amount: u64,
}

fn add_swap(state: &mut State, swap: SwapState) {
    state.swaps.insert(swap.state_id.to_string(), swap);
}

fn get_state(document: &AutoCommit) -> State {
    let state: State = hydrate(document).unwrap();

    state
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let server = Arc::new(Mutex::new(ServerDatabase::new()));
    let mut alice: EigensyncHandle<State> = EigensyncHandle::new(Arc::clone(&server));
    let mut bob: EigensyncHandle<State> = EigensyncHandle::new(Arc::clone(&server));

    let mut alice_state = get_state(&alice.document);

    add_swap(&mut alice_state, SwapState {
        state_id: Uuid::new_v4(),
        swap_id: 0,
        state: 0,
        amount: 300,
    });

    add_swap(&mut alice_state, SwapState {
        state_id: Uuid::new_v4(),
        swap_id: 1,
        state: 0,
        amount: 200,
    });

    alice.save_updates_local(&alice_state).unwrap();
    alice.sync_with_server().unwrap();
    
    let mut bob_state = get_state(&bob.document);
    
    bob.update_state(&mut bob_state).unwrap();
    
    assert_eq!(alice_state, bob_state, "bob got alice swaps");

    add_swap(&mut bob_state.clone(), SwapState {
        state_id: Uuid::new_v4(),
        swap_id: 1,
        state: 1,
        amount: 200,
    });

    bob.save_updates_local(&bob_state).unwrap();

    bob.sync_with_server().unwrap();

    alice.sync_with_server().unwrap();

    alice.update_state(&mut alice_state).unwrap();

    assert_eq!(
        alice_state, bob_state,
        "Alice and Bob should have the same state {:?}", bob_state
    );

    Ok(())
}