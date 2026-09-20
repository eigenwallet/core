CREATE TABLE IF NOT EXISTS wormholes (
    peer_id TEXT PRIMARY KEY NOT NULL,
    address TEXT NOT NULL,
    active BOOLEAN NOT NULL
);
