-- This migration adds tx_punish_fee and punish_address to all Bob state objects
-- that contain the "xmr" field (State3, State4, State5, State6).
-- These fields are needed to construct the punish transaction txid for explicit
-- punish detection, but were previously only stored in State2 (ExecutionSetupDone).
--
-- Uses json_tree to generically find the parent object of "xmr" regardless of nesting depth,
-- avoiding the need to enumerate each BobState variant individually.
-- json_insert is used (not json_set) so existing values are never overwritten.

UPDATE swap_states SET state = json_insert(
    json_insert(
        state,
        (SELECT jt.path || '.tx_punish_fee' FROM json_tree(swap_states.state) AS jt WHERE jt.key = 'xmr' LIMIT 1),
        (SELECT json_extract(s.state, '$.Bob.ExecutionSetupDone.state2.tx_punish_fee')
         FROM swap_states AS s
         WHERE s.swap_id = swap_states.swap_id
           AND json_extract(s.state, '$.Bob.ExecutionSetupDone') IS NOT NULL
         LIMIT 1)
    ),
    (SELECT jt.path || '.punish_address' FROM json_tree(swap_states.state) AS jt WHERE jt.key = 'xmr' LIMIT 1),
    (SELECT json_extract(s.state, '$.Bob.ExecutionSetupDone.state2.punish_address')
     FROM swap_states AS s
     WHERE s.swap_id = swap_states.swap_id
       AND json_extract(s.state, '$.Bob.ExecutionSetupDone') IS NOT NULL
     LIMIT 1)
)
WHERE json_extract(state, '$.Bob.ExecutionSetupDone') IS NULL
  AND (SELECT jt.path FROM json_tree(swap_states.state) AS jt WHERE jt.key = 'xmr' LIMIT 1) IS NOT NULL;
