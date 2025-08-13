CREATE UNIQUE INDEX IF NOT EXISTS swap_states_unique_over_state_and_timestamp
ON swap_states(state, entered_at);