-- This migration adds tx_punish_fee and punish_address to all Bob state objects
-- that contain the "xmr" field (State3, State4, State5, State6).
-- These fields are needed to construct the punish transaction txid for explicit
-- punish detection, but were previously only stored in State2 (ExecutionSetupDone).
--
-- Uses json_tree to generically find the parent object of "xmr" regardless of
-- nesting depth, avoiding the need to enumerate each BobState variant individually.
-- json_insert (not json_set) is used so existing values are never overwritten.

-- Step 1: Collect source values (tx_punish_fee, punish_address) per swap_id
-- from the ExecutionSetupDone state.
CREATE TEMP TABLE _punish_source AS
SELECT
    swap_id,
    json_extract(state, '$.Bob.ExecutionSetupDone.state2.tx_punish_fee') AS tx_punish_fee,
    json_extract(state, '$.Bob.ExecutionSetupDone.state2.punish_address') AS punish_address
FROM swap_states
WHERE json_extract(state, '$.Bob.ExecutionSetupDone') IS NOT NULL;

-- Assert: no NULL values in source table.
CREATE TABLE _assert (ok INTEGER NOT NULL CHECK(ok = 1));
INSERT INTO _assert
SELECT CASE WHEN COUNT(*) = 0 THEN 1 ELSE NULL END
FROM _punish_source
WHERE tx_punish_fee IS NULL OR punish_address IS NULL;
DROP TABLE _assert;

-- Step 2: Collect target rows and the JSON path where the new fields should be
-- inserted. json_tree's `path` column gives the parent object of the matched key,
-- so appending '.tx_punish_fee' inserts as a sibling of "xmr", not under it.
CREATE TEMP TABLE _punish_target AS
SELECT
    swap_states.id AS target_id,
    swap_states.swap_id,
    (SELECT jt.path FROM json_tree(swap_states.state) AS jt WHERE jt.key = 'xmr' LIMIT 1) AS parent_path
FROM swap_states
WHERE json_extract(state, '$.Bob.ExecutionSetupDone') IS NULL
  AND (SELECT jt.path FROM json_tree(swap_states.state) AS jt WHERE jt.key = 'xmr' LIMIT 1) IS NOT NULL;

-- Assert: every target row has a matching source row.
CREATE TABLE _assert (ok INTEGER NOT NULL CHECK(ok = 1));
INSERT INTO _assert
SELECT CASE WHEN COUNT(*) = 0 THEN 1 ELSE NULL END
FROM _punish_target AS t
WHERE NOT EXISTS (
    SELECT 1 FROM _punish_source AS src WHERE src.swap_id = t.swap_id
);
DROP TABLE _assert;

-- Step 3: Write the values into the state JSON.
UPDATE swap_states SET state = json_insert(
    json_insert(state, t.parent_path || '.tx_punish_fee', src.tx_punish_fee),
    t.parent_path || '.punish_address', src.punish_address
)
FROM _punish_target AS t
JOIN _punish_source AS src ON src.swap_id = t.swap_id
WHERE swap_states.id = t.target_id;

DROP TABLE _punish_target;
DROP TABLE _punish_source;
