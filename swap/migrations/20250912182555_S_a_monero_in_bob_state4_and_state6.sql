-- This migration makes S_a_monero available in Bob::{State4, State6} by copying it from Bob::State2.

UPDATE swap_states SET
    state = json_insert(
        state,
        '$.Bob.XmrLocked.state4.S_a_monero',
        (
            SELECT json_extract(states.state, '$.Bob.ExecutionSetupDone.state2.S_a_monero')
            FROM swap_states AS states
            WHERE
                states.swap_id = swap_states.swap_id
                AND json_extract(states.state, '$.Bob.ExecutionSetupDone') IS NOT NULL
            LIMIT 1
        )
    )
WHERE json_extract(state, '$.Bob.XmrLocked') IS NOT NULL;

UPDATE swap_states SET
    state = json_insert(
        state,
        '$.Bob.EncSigSent.state4.S_a_monero',
        (
            SELECT json_extract(states.state, '$.Bob.ExecutionSetupDone.state2.S_a_monero')
            FROM swap_states AS states
            WHERE
                states.swap_id = swap_states.swap_id
                AND json_extract(states.state, '$.Bob.ExecutionSetupDone') IS NOT NULL
            LIMIT 1
        )
    )
WHERE json_extract(state, '$.Bob.EncSigSent') IS NOT NULL;

UPDATE swap_states SET
    state = json_insert(
        state,
        '$.Bob.BtcPunished.state.S_a_monero',
        (
            SELECT json_extract(states.state, '$.Bob.ExecutionSetupDone.state2.S_a_monero')
            FROM swap_states AS states
            WHERE
                states.swap_id = swap_states.swap_id
                AND json_extract(states.state, '$.Bob.ExecutionSetupDone') IS NOT NULL
            LIMIT 1
        )
    )
WHERE json_extract(state, '$.Bob.BtcPunished') IS NOT NULL;

UPDATE swap_states SET
    state = json_insert(
        state,
        '$.Bob.CancelTimelockExpired.S_a_monero',
        (
            SELECT json_extract(states.state, '$.Bob.ExecutionSetupDone.state2.S_a_monero')
            FROM swap_states AS states
            WHERE
                states.swap_id = swap_states.swap_id
                AND json_extract(states.state, '$.Bob.ExecutionSetupDone') IS NOT NULL
            LIMIT 1
        )
    )
WHERE json_extract(state, '$.Bob.CancelTimelockExpired') IS NOT NULL;

UPDATE swap_states SET
    state = json_insert(
        state,
        '$.Bob.BtcCancelled.S_a_monero',
        (
            SELECT json_extract(states.state, '$.Bob.ExecutionSetupDone.state2.S_a_monero')
            FROM swap_states AS states
            WHERE
                states.swap_id = swap_states.swap_id
                AND json_extract(states.state, '$.Bob.ExecutionSetupDone') IS NOT NULL
            LIMIT 1
        )
    )
WHERE json_extract(state, '$.Bob.BtcCancelled') IS NOT NULL;

UPDATE swap_states SET
    state = json_insert(
        state,
        '$.Bob.BtcRefundPublished.S_a_monero',
        (
            SELECT json_extract(states.state, '$.Bob.ExecutionSetupDone.state2.S_a_monero')
            FROM swap_states AS states
            WHERE
                states.swap_id = swap_states.swap_id
                AND json_extract(states.state, '$.Bob.ExecutionSetupDone') IS NOT NULL
            LIMIT 1
        )
    )
WHERE json_extract(state, '$.Bob.BtcRefundPublished') IS NOT NULL;

UPDATE swap_states SET
    state = json_insert(
        state,
        '$.Bob.BtcEarlyRefundPublished.S_a_monero',
        (
            SELECT json_extract(states.state, '$.Bob.ExecutionSetupDone.state2.S_a_monero')
            FROM swap_states AS states
            WHERE
                states.swap_id = swap_states.swap_id
                AND json_extract(states.state, '$.Bob.ExecutionSetupDone') IS NOT NULL
            LIMIT 1
        )
    )
WHERE json_extract(state, '$.Bob.BtcEarlyRefundPublished') IS NOT NULL;
