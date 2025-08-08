pub mod protocol;
pub mod lib2;

use std::{marker::PhantomData, sync::{Arc, Mutex, OnceLock}};

use anyhow::Context;
use automerge::{ActorId, AutoCommit, Change};
use autosurgeon::{hydrate, reconcile, Hydrate, Reconcile};

// fn example() {
//     let mut state = State {};
//     let mut eigensync: Eigensync<State> = Eigensync::new();

//     eigensync.save_updates_local(&state);

//     eigensync.sync_with_server();

//     eigensync.update_state(&mut state);
// }

// hardcoded change that is used to initialize the document
pub static INIT_AUTOMERGE: OnceLock<AutoCommit> = OnceLock::new();

fn get_init_autocommit() -> AutoCommit {
    INIT_AUTOMERGE.get_or_init(|| AutoCommit::load(&[133, 111, 74, 131, 88, 3, 75, 84, 1, 48, 0, 
                                                    16, 139, 82, 195, 59, 223, 191, 74, 44, 186, 
                                                    159, 214, 214, 200, 14, 181, 60, 1, 1, 0, 0, 
                                                    0, 5, 21, 7, 52, 1, 66, 2, 86, 2, 112, 2, 
                                                    127, 5, 115, 119, 97, 112, 115, 1, 127, 
                                                    0, 127, 0, 127, 0]).unwrap().with_actor(ActorId::random()))
        .clone()
}

pub struct EigensyncHandle {
    sender: UnboundedSender<u64>,
}

pub struct Eigensync<T: Reconcile + Hydrate + Default> {
    pub document: AutoCommit,
    server: Arc<Mutex<ServerDatabase>>,
    _marker: PhantomData<T>,
}

impl<T: Reconcile + Hydrate + Default> Eigensync<T> {
    pub fn new(server: Arc<Mutex<ServerDatabase>>) -> Self {
        let (sender, receiver) = channel();

        task::spawn(async move {
            event_loop(receiver).await;
        }

        let mut document = get_init_autocommit();

        reconcile(&mut document, &T::default())
            .context("Failed to reconcile")
            .unwrap();

        Self { document, server, _marker: PhantomData }
    }

    pub fn save_updates_local(&mut self, state: &T) -> anyhow::Result<()> {
        reconcile(&mut self.document, state)
            .context("Failed to reconcile")?;
        
        Ok(())
    }

    pub fn sync_with_server(&mut self) -> anyhow::Result<()> {
        let new_changes = self.get_changes();

        let mut new_doc = self.document.fork();

        new_doc
            .apply_changes(new_changes.clone())
            .context("Failed to apply changes")?;

        self.document
            .merge(&mut new_doc)
            .context("Failed to merge")?;

        self.server.lock().unwrap().add_changes(new_changes)?;

        Ok(())
    }

    pub fn get_changes(&mut self) -> Vec<Change> {
        self.document
            .get_changes(&[])
            .iter()
            .map(|c| (*c).clone())
            .collect()
    }

    pub fn update_state(&mut self, state: &mut T) -> anyhow::Result<()> {
        let changes = self.server.lock().unwrap().get_changes(vec![]);

        self.document
            .apply_changes(changes)
            .context("Failed to apply changes")?;

        *state = hydrate(&self.document).unwrap();

        Ok(())
    }
}

#[derive(Clone)]
pub struct ServerDatabase {
    changes: Vec<Change>,
}

impl ServerDatabase {
    pub fn new() -> Self {
        Self { changes: vec![] }
    }

    pub fn add_changes(&mut self, changes: Vec<Change>) -> anyhow::Result<()> {
        let mut new_changes = vec![];

        for change in changes {
            if !self.changes.contains(&change) {
                new_changes.push(change);
            }
        }

        self.changes.extend_from_slice(&new_changes);

        Ok(())
    }

    pub fn get_changes(&mut self, changes: Vec<Change>) -> Vec<Change> {
        let mut new_changes = vec![];

        for change in self.changes.clone() {
            if !changes.contains(&change) {
                new_changes.push(change);
            }
        }

        new_changes
    }
}
