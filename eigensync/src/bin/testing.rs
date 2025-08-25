use std::collections::HashMap;

use automerge::AutoCommit;
use autosurgeon::{hydrate, Hydrate, Reconcile};
use eigensync::EigensyncHandle;
use libp2p::{identity, Multiaddr, PeerId};
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
    let server_addr = "/ip4/127.0.0.1/tcp/9999".parse::<Multiaddr>()?;
    let server_id = PeerId::random();
    let alice_keypair = identity::Keypair::generate_ed25519();
    let bob_keypair = identity::Keypair::generate_ed25519();

    let mut alice: EigensyncHandle<State> = EigensyncHandle::new(server_addr.clone(), server_id, alice_keypair).await?;
    let mut bob: EigensyncHandle<State> = EigensyncHandle::new(server_addr, server_id, bob_keypair).await?;

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

    
    alice.sync_with_server().await?;
    
    bob.sync_with_server().await?;
    let bob_state = get_state(&bob.document);
    
    assert_eq!(alice_state, bob_state, "bob got alice swaps");

    // Move the add_swap call into modify closure above

    bob.modify(|state| {
        add_swap(state, SwapState {
            state_id: Uuid::new_v4(),
            swap_id: 1,
            state: 1,
            amount: 200,
        });
        Ok(())
    })?;

    bob.sync_with_server().await?;

    alice.sync_with_server().await?;

    alice_state = get_state(&alice.document);

    let bob_state = get_state(&bob.document);
    
    assert_eq!(
        alice_state, bob_state,
        "Alice and Bob should have the same state {:?}", bob_state
    );

    Ok(())
}