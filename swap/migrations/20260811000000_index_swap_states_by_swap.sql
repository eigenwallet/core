CREATE INDEX IF NOT EXISTS idx_swap_states_swap_id_id
ON swap_states (swap_id, id DESC);
