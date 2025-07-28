-- With Monero output management, we might need more inputs to a lock transaction than fit into 
-- just one transaction. Thus we need to be able to handle multiple transfer proofs (one for each tx).

-- This migration removes the primary key on buffered_transfer_proofs.swap_id 
-- and instead creates an index on swap_id. This allows multiple proofs per swap id.

-- We also need to update basically all states.

ALTER TABLE buffered_transfer_proofs RENAME TO buffered_transfer_proofs_single;

-- Keep old table in case we need to rollback
CREATE TABLE buffered_transfer_proofs (
    swap_id TEXT NOT NULL,
    proof TEXT NOT NULL
);

-- Copy data from the old table to the new table
INSERT INTO buffered_transfer_proofs (swap_id, proof)
SELECT swap_id, proof FROM buffered_transfer_proofs_single;

-- Create an index on swap_id -- still prevent exact duplicates but multiple different transfer proofs per swap are fine
CREATE UNIQUE INDEX idx_buffered_transfer_proofs_swap_id ON buffered_transfer_proofs(swap_id);

-- Now we iterate over all states which contain a transfer proof and update the field to contain a list of transfer proofs instead.


ALTER TABLE swap_states RENAME TO swap_states_single_transfer_proof;

CREATE TABLE swap_states
(
    id          INTEGER PRIMARY KEY autoincrement NOT NULL,
    swap_id     TEXT                NOT NULL,
    entered_at  TEXT                NOT NULL,
    state       TEXT                NOT NULL
);


-- First copy over old state data
INSERT INTO swap_states (id, swap_id, entered_at, state) 
SELECT id, swap_id, entered_at, state FROM swap_states_single_transfer_proof;

-- For every state that contains a transfer_proof or lock_transfer_proof object,
-- convert that field to contain an array instead.

UPDATE swap_states 
SET state = json_set(
    state,
    (
        -- We override the transfer_proof or lock_transfer_proof field (there is at most one per state)
        SELECT tree.fullkey 
        FROM json_tree(swap_states.state) tree 
        WHERE (tree.key = 'transfer_proof' OR tree.key = 'lock_transfer_proof') 
          AND tree.type = 'object'
        LIMIT 1
    ),
    json_array(
        -- We override it with an array containing the proof instead of a raw object
        json_extract(
            state, 
            (
                SELECT tree.fullkey 
                FROM json_tree(swap_states.state) tree 
                WHERE (tree.key = 'transfer_proof' OR tree.key = 'lock_transfer_proof') 
                  AND tree.type = 'object'
                LIMIT 1
            )
        )
    )
)
-- We only do this for states that contain a transfer_proof or lock_transfer_proof object.
WHERE EXISTS (
    SELECT 1 FROM json_tree(state) tree
    WHERE (tree.key = 'transfer_proof' OR tree.key = 'lock_transfer_proof') 
      AND tree.type = 'object'
);
