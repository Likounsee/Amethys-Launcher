-- Distributed Store master catalogue only.
-- Never add per-user installations, local paths, ownership or download state here.
PRAGMA foreign_keys = ON;

CREATE TABLE games (
    igdb_id          INTEGER PRIMARY KEY,
    amethys_id       INTEGER NOT NULL UNIQUE CHECK (amethys_id > 0),
    name             TEXT NOT NULL,
    slug             TEXT,
    summary          TEXT,
    release_date     TEXT,
    cover_url        TEXT,
    genres_json      TEXT NOT NULL DEFAULT '[]',
    updated_at       TEXT NOT NULL
);

CREATE TABLE game_stores (
    id               INTEGER PRIMARY KEY,
    igdb_game_id     INTEGER NOT NULL REFERENCES games(igdb_id) ON DELETE CASCADE,
    store            TEXT NOT NULL,
    external_id      TEXT NOT NULL,
    store_name       TEXT,
    store_url        TEXT NOT NULL,
    UNIQUE (store, external_id),
    CHECK (length(store) > 0),
    CHECK (length(external_id) > 0)
);

CREATE TABLE catalog_meta (
    key              TEXT PRIMARY KEY,
    value            TEXT NOT NULL
);

CREATE INDEX idx_games_amethys_id ON games(amethys_id);
CREATE INDEX idx_games_name ON games(name COLLATE NOCASE);
CREATE INDEX idx_games_slug ON games(slug);
CREATE INDEX idx_game_stores_game ON game_stores(igdb_game_id);
CREATE INDEX idx_game_stores_store_id ON game_stores(store, external_id);
CREATE INDEX idx_game_stores_external_id ON game_stores(external_id);
