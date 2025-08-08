use std::{collections::HashMap, str::FromStr, time::Duration};

use anyhow::Context;
use automerge::{AutoCommit};
use autosurgeon::{hydrate, Hydrate, Reconcile};
use eigensync::EigensyncHandle;
use libp2p::{
    Multiaddr, PeerId,
};
use uuid::Uuid;

// pub struct Database {
//     document: AutoCommit,
//     state: State,
// }

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

#[derive(Debug, Clone, Reconcile, Hydrate, PartialEq)]
pub struct Swap {
    #[key]
    pub id: Uuid,
    pub amount: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let multiaddr = Multiaddr::from_str("/ip4/127.0.0.1/tcp/3333").context("")?;
    let server_peer_id = PeerId::from_str("12D3KooWQsAFHUm32ThqfQRJhtcc57qqkYckSu8JkMsbGKkwTS6p")?;

    let mut eigensync = EigensyncHandle::<State>::new(multiaddr, server_peer_id).unwrap();

    eigensync.modify(|state| {
        add_swap(state, SwapState {
            state_id: Uuid::new_v4(),
            swap_id: 1,
            state: 0,
            amount: 400,
        });

        Ok(())
    }).await.unwrap();

    for _ in 0..10 {
        eigensync.modify(|state| {
            add_swap(state, SwapState {
                state_id: Uuid::new_v4(),
                swap_id: 1,
                state: 0,
                amount: 400,
            });
    
            Ok(())
        }).await.unwrap();
        let _ = eigensync.save_and_sync().await.inspect_err(|e| eprintln!("Error: {:?}", e));
        tokio::time::sleep(Duration::from_millis(200)).await;
    };

    Ok(())
}