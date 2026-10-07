CREATE TABLE IF NOT EXISTS swap_attestations
(
    swap_id     TEXT    PRIMARY KEY NOT NULL,
    attestation TEXT                NOT NULL,
    entered_at  TEXT                NOT NULL
);
