use amethys_errors::{AmethysError, ErrorCode};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;

/// A game registered in the user's local library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedGameEntry {
    pub amethys_id: u64,
    pub name: String,
    pub genres: Vec<String>,
    pub added_at: String,
}

/// A game registered as owned, independently from any local installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameEntry {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub platform_id: Option<String>,
    pub install_path: String,
    pub executable: Option<String>,
    pub added_at: String,
}

/// A canonical catalogue record shared by multiple local installations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogueGame {
    pub id: String,
    pub name: String,
}

/// Amethys-owned metadata shared by every storefront identity for one game.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GameMetadata {
    pub genres: Vec<String>,
    pub developer: Option<String>,
    pub publisher: Option<String>,
    pub release_date: Option<String>,
    pub description: Option<String>,
    pub icon_ref: Option<String>,
    pub logo_ref: Option<String>,
    pub cover_ref: Option<String>,
    pub banner_ref: Option<String>,
}

/// An external storefront identifier attached to an Amethys game ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalGameId {
    pub provider: String,
    pub external_id: String,
}

const MAX_GAME_ID_BYTES: usize = 128;
const MAX_GAME_NAME_BYTES: usize = 512;
const MAX_PLATFORM_NAME_BYTES: usize = 128;
const MAX_PLATFORM_ID_BYTES: usize = 512;
const MAX_GAME_PATH_BYTES: usize = 32 * 1024;

/// Enables WAL while tolerating another process opening the same new database.
fn enable_wal_mode(connection: &Connection) -> rusqlite::Result<()> {
    const RETRY_WINDOW: std::time::Duration = std::time::Duration::from_secs(5);
    const BUSY_SLICE: std::time::Duration = std::time::Duration::from_millis(100);

    // A long SQLite busy timeout would consume the entire retry window in a
    // single attempt. Use short waits here so we can retry until the deadline.
    connection.busy_timeout(BUSY_SLICE)?;
    let deadline = std::time::Instant::now() + RETRY_WINDOW;
    let result = loop {
        match connection.pragma_update(None, "journal_mode", "WAL") {
            // rusqlite's update API uses SQLite's exec interface, which is
            // required for journal_mode because the pragma returns a row.
            Ok(()) => break Ok(()),
            Err(error)
                if matches!(
                    &error,
                    rusqlite::Error::SqliteFailure(code, _)
                        if matches!(
                            code.code,
                            rusqlite::ffi::ErrorCode::DatabaseBusy
                                | rusqlite::ffi::ErrorCode::DatabaseLocked
                        )
                ) && std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(error) => break Err(error),
        }
    };

    // Restore the normal timeout regardless of whether WAL initialization
    // succeeded, so subsequent migrations and queries retain expected behavior.
    let restore_timeout = connection.busy_timeout(std::time::Duration::from_secs(5));
    result.and(restore_timeout)
}

/// SQLite database owned by the core process.
pub struct Database {
    connection: Connection,
}

impl Database {
    /// Opens or creates the database and applies all known schema migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, AmethysError> {
        let path = path.as_ref();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|error| {
                AmethysError::new(
                    ErrorCode::StorageOpen,
                    format!("Could not create {}: {error}", parent.display()),
                )
            })?;
        }
        let mut connection = Connection::open(path).map_err(|error| {
            AmethysError::new(
                ErrorCode::StorageOpen,
                format!("Could not open {}: {error}", path.display()),
            )
        })?;
        // A second Amethys process or another database client may briefly hold
        // a write lock. Wait for it instead of failing immediately on SQLITE_BUSY.
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|error| {
                AmethysError::new(
                    ErrorCode::StorageOpen,
                    format!("Could not configure database busy timeout: {error}"),
                )
            })?;
        // Check the schema before changing persistent SQLite settings. In
        // particular, opening a database created by a newer Amethys release
        // must not silently switch its journal mode or otherwise mutate it.
        let migration_table_exists: bool = connection
            .query_row(
                "SELECT EXISTS(
                 SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'schema_migrations'
             )",
                [],
                |row| row.get(0),
            )
            .map_err(|error| {
                AmethysError::new(
                    ErrorCode::StorageMigration,
                    format!("Could not inspect database schema: {error}"),
                )
            })?;
        let detected_version: i64 = if migration_table_exists {
            connection
                .query_row(
                    "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
                    [],
                    |row| row.get(0),
                )
                .map_err(|error| {
                    AmethysError::new(
                        ErrorCode::StorageMigration,
                        format!("Could not read migration history: {error}"),
                    )
                })?
        } else {
            0
        };
        if detected_version > 7 {
            return Err(AmethysError::new(
                ErrorCode::StorageMigration,
                format!(
                    "Database schema version {detected_version} is newer than the supported version 7."
                ),
            ));
        }

        connection
            .execute_batch(
                "PRAGMA foreign_keys = ON;
             PRAGMA synchronous = FULL;",
            )
            .map_err(|error| {
                AmethysError::new(
                    ErrorCode::StorageOpen,
                    format!("Could not configure database: {error}"),
                )
            })?;
        enable_wal_mode(&connection).map_err(|error| {
            AmethysError::new(
                ErrorCode::StorageOpen,
                format!("Could not enable database WAL mode: {error}"),
            )
        })?;

        // Keep schema creation and migration markers in one transaction. A
        // failed statement must not leave a partially initialized database.
        let transaction = connection.transaction().map_err(|error| {
            AmethysError::new(
                ErrorCode::StorageMigration,
                format!("Could not begin database migration: {error}"),
            )
        })?;
        transaction
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS schema_migrations (
                 version INTEGER PRIMARY KEY NOT NULL,
                 applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );",
            )
            .map_err(|error| {
                AmethysError::new(
                    ErrorCode::StorageMigration,
                    format!("Could not create migration history: {error}"),
                )
            })?;

        // Re-read under the migration transaction in case another Amethys
        // process completed a migration after the read-only preflight.
        let current_version: i64 = transaction
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
                [],
                |row| row.get(0),
            )
            .map_err(|error| {
                AmethysError::new(
                    ErrorCode::StorageMigration,
                    format!("Could not read migration history: {error}"),
                )
            })?;
        if current_version > 7 {
            return Err(AmethysError::new(
                ErrorCode::StorageMigration,
                format!(
                    "Database schema version {current_version} is newer than the supported version 7."
                ),
            ));
        }

        transaction
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS app_settings (
                 key TEXT PRIMARY KEY NOT NULL,
                 value TEXT NOT NULL
             );
             INSERT OR IGNORE INTO schema_migrations (version) VALUES (1);
             CREATE TABLE IF NOT EXISTS games (
                 id TEXT PRIMARY KEY NOT NULL,
                 name TEXT NOT NULL,
                 platform TEXT NOT NULL,
                 install_path TEXT NOT NULL,
                 executable TEXT,
                 added_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );
             CREATE INDEX IF NOT EXISTS idx_games_name ON games(name COLLATE NOCASE);
             INSERT OR IGNORE INTO schema_migrations (version) VALUES (2);",
            )
            .map_err(|error| {
                AmethysError::new(
                    ErrorCode::StorageMigration,
                    format!("Could not initialize database schema: {error}"),
                )
            })?;
        if current_version < 3 {
            transaction
                .execute_batch(
                    "ALTER TABLE games ADD COLUMN platform_id TEXT;
                 CREATE UNIQUE INDEX IF NOT EXISTS idx_games_platform_identity
                     ON games(platform COLLATE NOCASE, platform_id)
                     WHERE platform_id IS NOT NULL;
                 INSERT OR IGNORE INTO schema_migrations (version) VALUES (3);",
                )
                .map_err(|error| {
                    AmethysError::new(
                        ErrorCode::StorageMigration,
                        format!("Could not apply game platform-ID migration: {error}"),
                    )
                })?;
        }
        if current_version < 4 {
            transaction
                .execute_batch(
                    "CREATE TABLE IF NOT EXISTS game_metadata (
                     game_id TEXT PRIMARY KEY NOT NULL REFERENCES games(id) ON DELETE CASCADE,
                     genres_json TEXT NOT NULL DEFAULT '[]',
                     developer TEXT,
                     publisher TEXT,
                     release_date TEXT,
                     description TEXT,
                     icon_ref TEXT,
                     logo_ref TEXT,
                     cover_ref TEXT,
                     banner_ref TEXT
                 );
                 CREATE TABLE IF NOT EXISTS game_external_ids (
                     game_id TEXT NOT NULL REFERENCES games(id) ON DELETE CASCADE,
                     provider TEXT NOT NULL COLLATE NOCASE,
                     external_id TEXT NOT NULL,
                     PRIMARY KEY (game_id, provider),
                     UNIQUE (provider, external_id)
                 );
                 CREATE INDEX IF NOT EXISTS idx_game_external_ids_game
                     ON game_external_ids(game_id);
                 INSERT OR IGNORE INTO schema_migrations (version) VALUES (4);",
                )
                .map_err(|error| {
                    AmethysError::new(
                        ErrorCode::StorageMigration,
                        format!("Could not apply game metadata migration: {error}"),
                    )
                })?;
        }
        if current_version < 5 {
            transaction
                .execute_batch(
                    "CREATE TABLE catalogue_games (
                     id TEXT PRIMARY KEY NOT NULL,
                     name TEXT NOT NULL
                 );
                 ALTER TABLE games ADD COLUMN catalogue_game_id TEXT
                     REFERENCES catalogue_games(id) ON DELETE SET NULL;
                 CREATE TEMP TABLE _amethys_catalogue_migration (
                     installation_id TEXT PRIMARY KEY NOT NULL,
                     catalogue_game_id TEXT NOT NULL
                 );
                 INSERT INTO _amethys_catalogue_migration (installation_id, catalogue_game_id)
                     SELECT id, CAST(2999 + ROW_NUMBER() OVER (ORDER BY id) AS TEXT) FROM games;
                 INSERT INTO catalogue_games (id, name)
                     SELECT migration.catalogue_game_id, games.name
                     FROM games
                     INNER JOIN _amethys_catalogue_migration AS migration
                         ON migration.installation_id = games.id;
                 UPDATE games
                     SET catalogue_game_id = (
                         SELECT catalogue_game_id
                         FROM _amethys_catalogue_migration
                         WHERE installation_id = games.id
                     );
                 CREATE TABLE catalogue_game_metadata (
                     catalogue_game_id TEXT PRIMARY KEY NOT NULL
                         REFERENCES catalogue_games(id) ON DELETE CASCADE,
                     genres_json TEXT NOT NULL DEFAULT '[]',
                     developer TEXT,
                     publisher TEXT,
                     release_date TEXT,
                     description TEXT,
                     icon_ref TEXT,
                     logo_ref TEXT,
                     cover_ref TEXT,
                     banner_ref TEXT
                 );
                 INSERT INTO catalogue_game_metadata (
                     catalogue_game_id, genres_json, developer, publisher, release_date,
                     description, icon_ref, logo_ref, cover_ref, banner_ref
                 )
                     SELECT migration.catalogue_game_id, metadata.genres_json,
                         metadata.developer, metadata.publisher, metadata.release_date,
                         metadata.description, metadata.icon_ref, metadata.logo_ref,
                         metadata.cover_ref, metadata.banner_ref
                     FROM game_metadata AS metadata
                     INNER JOIN _amethys_catalogue_migration AS migration
                         ON migration.installation_id = metadata.game_id;
                 CREATE TABLE catalogue_external_ids (
                     catalogue_game_id TEXT NOT NULL
                         REFERENCES catalogue_games(id) ON DELETE CASCADE,
                     provider TEXT NOT NULL COLLATE NOCASE,
                     external_id TEXT NOT NULL,
                     PRIMARY KEY (catalogue_game_id, provider),
                     UNIQUE (provider, external_id)
                 );
                 INSERT INTO catalogue_external_ids (catalogue_game_id, provider, external_id)
                     SELECT migration.catalogue_game_id, external.provider, external.external_id
                     FROM game_external_ids AS external
                     INNER JOIN _amethys_catalogue_migration AS migration
                         ON migration.installation_id = external.game_id;
                 CREATE INDEX idx_catalogue_external_ids_game
                     ON catalogue_external_ids(catalogue_game_id);
                 DROP TABLE game_metadata;
                 DROP TABLE game_external_ids;
                 DROP TABLE _amethys_catalogue_migration;
                 INSERT OR IGNORE INTO schema_migrations (version) VALUES (5);",
                )
                .map_err(|error| {
                    AmethysError::new(
                        ErrorCode::StorageMigration,
                        format!("Could not apply canonical catalogue-ID migration: {error}"),
                    )
                })?;
        }

        if current_version < 6 {
            transaction
                .execute_batch(
                    "CREATE TABLE local_game_metadata (
                     game_id TEXT PRIMARY KEY NOT NULL REFERENCES games(id) ON DELETE CASCADE,
                     genres_json TEXT NOT NULL DEFAULT '[]',
                     developer TEXT, publisher TEXT, release_date TEXT, description TEXT,
                     icon_ref TEXT, logo_ref TEXT, cover_ref TEXT, banner_ref TEXT
                 );
                 INSERT INTO local_game_metadata (
                     game_id, genres_json, developer, publisher, release_date,
                     description, icon_ref, logo_ref, cover_ref, banner_ref
                 )
                     SELECT games.id, metadata.genres_json, metadata.developer,
                         metadata.publisher, metadata.release_date, metadata.description,
                         metadata.icon_ref, metadata.logo_ref, metadata.cover_ref, metadata.banner_ref
                     FROM games
                     INNER JOIN catalogue_game_metadata AS metadata
                         ON metadata.catalogue_game_id = games.catalogue_game_id;
                 UPDATE games SET catalogue_game_id = NULL;
                 DELETE FROM catalogue_external_ids;
                 DELETE FROM catalogue_game_metadata;
                 DELETE FROM catalogue_games;
                 ALTER TABLE catalogue_games ADD COLUMN amethys_id INTEGER;
                 ALTER TABLE catalogue_games ADD COLUMN retired INTEGER NOT NULL DEFAULT 0
                     CHECK (retired IN (0, 1));
                 CREATE UNIQUE INDEX idx_catalogue_games_amethys_id
                     ON catalogue_games(amethys_id) WHERE amethys_id IS NOT NULL;
                 CREATE TABLE local_game_locations (
                     id TEXT PRIMARY KEY NOT NULL,
                     game_id TEXT NOT NULL REFERENCES games(id) ON DELETE CASCADE,
                     path TEXT NOT NULL,
                     is_primary INTEGER NOT NULL DEFAULT 1 CHECK (is_primary IN (0, 1)),
                     UNIQUE(game_id, path)
                 );
                 CREATE INDEX idx_local_game_locations_game ON local_game_locations(game_id);
                 INSERT INTO local_game_locations (id, game_id, path, is_primary)
                     SELECT 'legacy-' || id, id, install_path, 1 FROM games;
                 INSERT OR IGNORE INTO schema_migrations (version) VALUES (6);",
                )
                .map_err(|error| {
                    AmethysError::new(
                        ErrorCode::StorageMigration,
                        format!("Could not apply global/local catalogue separation migration: {error}"),
                    )
                })?;
        }
        if current_version < 7 {
            transaction
                .execute_batch(
                    "CREATE TABLE owned_games (
                    amethys_id INTEGER PRIMARY KEY NOT NULL CHECK (amethys_id > 0),
                    name TEXT NOT NULL,
                    genres_json TEXT NOT NULL DEFAULT '[]',
                    added_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );
                CREATE INDEX idx_owned_games_name ON owned_games(name COLLATE NOCASE);
                INSERT OR IGNORE INTO schema_migrations (version) VALUES (7);",
                )
                .map_err(|error| {
                    AmethysError::new(
                        ErrorCode::StorageMigration,
                        format!("Could not apply owned-games library migration: {error}"),
                    )
                })?;
        }
        transaction.commit().map_err(|error| {
            AmethysError::new(
                ErrorCode::StorageMigration,
                format!("Could not commit database migration: {error}"),
            )
        })?;
        Ok(Self { connection })
    }

    /// Adds a game without a platform-specific identifier.
    pub fn add_game(
        &self,
        name: &str,
        platform: &str,
        install_path: &str,
        executable: Option<&str>,
    ) -> Result<GameEntry, AmethysError> {
        self.add_game_with_platform_id(name, platform, None, install_path, executable)
    }

    /// Adds a game while preserving its identifier in the source platform.
    pub fn add_game_with_platform_id(
        &self,
        name: &str,
        platform: &str,
        platform_id: Option<&str>,
        install_path: &str,
        executable: Option<&str>,
    ) -> Result<GameEntry, AmethysError> {
        let name = name.trim();
        let platform = platform.trim();
        let install_path = install_path.trim();
        let platform_id = platform_id.map(str::trim);
        let executable = executable.map(str::trim).filter(|value| !value.is_empty());
        if name.is_empty() || platform.is_empty() || install_path.is_empty() {
            return Err(storage_error(
                "Game name, platform and install path must not be empty.",
            ));
        }
        if platform_id.is_some_and(str::is_empty) {
            return Err(storage_error(
                "Platform game ID must not be empty when provided.",
            ));
        }
        // SQLite accepts embedded NUL bytes in TEXT values, but downstream
        // filesystem and process APIs cannot use such paths safely. Reject
        // them at the persistence boundary rather than storing unusable data.
        if [name, platform, install_path]
            .iter()
            .any(|value| value.contains('\0'))
            || platform_id.is_some_and(|value| value.contains('\0'))
            || executable.is_some_and(|value| value.contains('\0'))
        {
            return Err(storage_error(
                "Game metadata must not contain NUL characters.",
            ));
        }
        if name.len() > MAX_GAME_NAME_BYTES {
            return Err(storage_error(format!(
                "Game name exceeds the {MAX_GAME_NAME_BYTES}-byte limit."
            )));
        }
        if platform.len() > MAX_PLATFORM_NAME_BYTES {
            return Err(storage_error(format!(
                "Platform name exceeds the {MAX_PLATFORM_NAME_BYTES}-byte limit."
            )));
        }
        if platform_id.is_some_and(|value| value.len() > MAX_PLATFORM_ID_BYTES) {
            return Err(storage_error(format!(
                "Platform game ID exceeds the {MAX_PLATFORM_ID_BYTES}-byte limit."
            )));
        }
        if install_path.len() > MAX_GAME_PATH_BYTES
            || executable.is_some_and(|value| value.len() > MAX_GAME_PATH_BYTES)
        {
            return Err(storage_error(format!(
                "Game paths must not exceed {MAX_GAME_PATH_BYTES} bytes."
            )));
        }

        let id = uuid::Uuid::new_v4().to_string();
        let transaction = self.connection.unchecked_transaction().map_err(|error| {
            storage_error(format!("Could not begin local installation insert: {error}"))
        })?;
        transaction
            .execute(
                "INSERT INTO games (
                    id, name, platform, platform_id, install_path, executable, catalogue_game_id
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)",
                params![id, name, platform, platform_id, install_path, executable],
            )
            .map_err(|error| storage_error(format!("Could not add local installation: {error}")))?;
        sync_primary_game_location(&transaction, &id, install_path)?;
        transaction.commit().map_err(|error| {
            storage_error(format!("Could not commit local installation insert: {error}"))
        })?;
        self.get_game(&id)?
            .ok_or_else(|| storage_error("Added game could not be read back."))
    }

    /// Finds a game using the identifier assigned by its source platform.
    pub fn get_game_by_platform_id(
        &self,
        platform: &str,
        platform_id: &str,
    ) -> Result<Option<GameEntry>, AmethysError> {
        let (platform, platform_id) = validate_platform_identity(platform, platform_id)?;
        self.connection
            .query_row(
                "SELECT id, name, platform, platform_id, install_path, executable, added_at
             FROM games WHERE platform = ?1 COLLATE NOCASE AND platform_id = ?2",
                params![platform, platform_id],
                game_from_row,
            )
            .optional()
            .map_err(|error| storage_error(format!("Could not find game by platform ID: {error}")))
    }

    /// Finds one game by its stable identifier.
    pub fn get_game(&self, id: &str) -> Result<Option<GameEntry>, AmethysError> {
        let id = validate_game_id(id)?;
        self.connection
            .query_row(
                "SELECT id, name, platform, platform_id, install_path, executable, added_at
             FROM games WHERE id = ?1",
                [id],
                game_from_row,
            )
            .optional()
            .map_err(|error| storage_error(format!("Could not read game: {error}")))
    }

    /// Returns the library ordered by name, without depending on a platform client.
    pub fn list_games(&self) -> Result<Vec<GameEntry>, AmethysError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, name, platform, platform_id, install_path, executable, added_at
             FROM games ORDER BY name COLLATE NOCASE, id",
            )
            .map_err(|error| storage_error(format!("Could not prepare game list: {error}")))?;
        let rows = statement
            .query_map([], game_from_row)
            .map_err(|error| storage_error(format!("Could not list games: {error}")))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| storage_error(format!("Could not read game list: {error}")))
    }

    /// Attaches a source-platform identifier to a legacy game entry.
    ///
    /// This is used when a previously imported game had no stable platform ID
    /// but a later discovery scan can identify it. The platform must match and
    /// an existing identifier is never overwritten.
    pub fn set_game_platform_id(
        &self,
        id: &str,
        platform: &str,
        platform_id: &str,
    ) -> Result<Option<GameEntry>, AmethysError> {
        let id = validate_game_id(id)?;
        let (platform, platform_id) = validate_platform_identity(platform, platform_id)?;
        let changed = self
            .connection
            .execute(
                "UPDATE games SET platform_id = ?1
             WHERE id = ?2 AND platform = ?3 COLLATE NOCASE AND platform_id IS NULL",
                params![platform_id, id, platform],
            )
            .map_err(|error| {
                storage_error(format!("Could not attach game platform ID: {error}"))
            })?;

        if changed == 0 {
            // Make retries idempotent, but never report success for a conflicting
            // platform or an already assigned, different platform identifier.
            return Ok(self.get_game(id)?.filter(|game| {
                game.platform.eq_ignore_ascii_case(platform)
                    && game.platform_id.as_deref() == Some(platform_id)
            }));
        }

        self.get_game(id)
    }

    /// Saves Amethys metadata without changing the stable local Game ID.
    pub fn set_game_metadata(
        &self,
        game_id: &str,
        metadata: &GameMetadata,
    ) -> Result<bool, AmethysError> {
        let game_id = validate_game_id(game_id)?;
        if metadata.genres.len() > 64 {
            return Err(storage_error("A game cannot have more than 64 genres."));
        }
        let genres = metadata
            .genres
            .iter()
            .map(|value| value.trim())
            .collect::<Vec<_>>();
        if genres
            .iter()
            .any(|value| value.is_empty() || value.len() > 128 || value.contains('\0'))
        {
            return Err(storage_error(
                "Genre names must be non-empty, NUL-free and at most 128 bytes.",
            ));
        }
        let genres_json = serde_json::to_string(&genres)
            .map_err(|error| storage_error(format!("Could not encode game genres: {error}")))?;
        let game_exists: bool = self
            .connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM games WHERE id = ?1)",
                [game_id],
                |row| row.get(0),
            )
            .map_err(|error| storage_error(format!("Could not check local game: {error}")))?;
        if !game_exists {
            return Ok(false);
        }
        let optional_fields = [
            (&metadata.developer, 512, "developer"),
            (&metadata.publisher, 512, "publisher"),
            (&metadata.release_date, 32, "release date"),
            (&metadata.description, 32 * 1024, "description"),
            (&metadata.icon_ref, 2048, "icon reference"),
            (&metadata.logo_ref, 2048, "logo reference"),
            (&metadata.cover_ref, 2048, "cover reference"),
            (&metadata.banner_ref, 2048, "banner reference"),
        ];
        for (value, max_bytes, label) in optional_fields {
            if value
                .as_ref()
                .is_some_and(|value| value.contains('\0') || value.len() > max_bytes)
            {
                return Err(storage_error(format!(
                    "Game {label} is invalid or exceeds {max_bytes} bytes."
                )));
            }
        }
        let changed = self
            .connection
            .execute(
                "INSERT INTO local_game_metadata (
                game_id, genres_json, developer, publisher, release_date, description,
                icon_ref, logo_ref, cover_ref, banner_ref
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(game_id) DO UPDATE SET
                genres_json = excluded.genres_json,
                developer = excluded.developer,
                publisher = excluded.publisher,
                release_date = excluded.release_date,
                description = excluded.description,
                icon_ref = excluded.icon_ref,
                logo_ref = excluded.logo_ref,
                cover_ref = excluded.cover_ref,
                banner_ref = excluded.banner_ref",
                params![
                    game_id,
                    genres_json,
                    metadata.developer,
                    metadata.publisher,
                    metadata.release_date,
                    metadata.description,
                    metadata.icon_ref,
                    metadata.logo_ref,
                    metadata.cover_ref,
                    metadata.banner_ref
                ],
            )
            .map_err(|error| {
                storage_error(format!("Could not save local game metadata: {error}"))
            })?;
        Ok(changed != 0)
    }

    /// Reads metadata through a local installation ID.
    pub fn get_game_metadata(&self, game_id: &str) -> Result<Option<GameMetadata>, AmethysError> {
        let game_id = validate_game_id(game_id)?;
        self.connection
            .query_row(
                "SELECT genres_json, developer, publisher, release_date, description,
                    icon_ref, logo_ref, cover_ref, banner_ref
                 FROM local_game_metadata WHERE game_id = ?1",
                [game_id],
                |row| {
                    let genres_json: String = row.get(0)?;
                    let genres = serde_json::from_str(&genres_json).map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })?;
                    Ok(GameMetadata {
                        genres,
                        developer: row.get(1)?,
                        publisher: row.get(2)?,
                        release_date: row.get(3)?,
                        description: row.get(4)?,
                        icon_ref: row.get(5)?,
                        logo_ref: row.get(6)?,
                        cover_ref: row.get(7)?,
                        banner_ref: row.get(8)?,
                    })
                },
            )
            .optional()
            .map_err(|error| storage_error(format!("Could not read local game metadata: {error}")))
    }

    /// Registers a game from the authoritative shared Amethys catalogue.
    /// Local discovery/import must never call this method.
    pub fn register_catalogue_game(
        &self,
        catalogue_game_id: &str,
        amethys_id: u64,
        name: &str,
        retired: bool,
    ) -> Result<(), AmethysError> {
        let catalogue_game_id = validate_game_id(catalogue_game_id)?;
        let name = name.trim();
        if amethys_id == 0
            || amethys_id > i64::MAX as u64
            || name.is_empty()
            || name.len() > MAX_GAME_NAME_BYTES
            || name.contains('\0')
        {
            return Err(storage_error(
                "Master catalogue identity or name is invalid.",
            ));
        }
        let id = catalogue_game_id.to_owned();
        let amethys_id_sql = amethys_id as i64;
        self.connection
            .execute(
                "INSERT INTO catalogue_games (id, name, amethys_id, retired)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, retired = excluded.retired
             WHERE catalogue_games.amethys_id = excluded.amethys_id",
                params![id, name, amethys_id_sql, retired],
            )
            .map_err(|error| {
                storage_error(format!("Could not register master catalogue game: {error}"))
            })?;
        let stored: Option<i64> = self
            .connection
            .query_row(
                "SELECT amethys_id FROM catalogue_games WHERE id = ?1",
                [&id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| {
                storage_error(format!("Could not verify master catalogue ID: {error}"))
            })?;
        if stored != Some(amethys_id as i64) {
            return Err(storage_error(
                "Amethys ID is already assigned to a different master catalogue entry.",
            ));
        }
        Ok(())
    }

    /// Adds a verified platform identity to an existing master catalogue game.
    pub fn set_catalogue_external_id(
        &self,
        amethys_id: u64,
        provider: &str,
        external_id: &str,
    ) -> Result<bool, AmethysError> {
        let provider = validate_provider_key(provider)?;
        let external_id = external_id.trim();
        if amethys_id == 0
            || amethys_id > i64::MAX as u64
            || external_id.is_empty()
            || external_id.len() > MAX_PLATFORM_ID_BYTES
            || external_id.contains('\0')
        {
            return Err(storage_error(
                "Master catalogue external identity is invalid.",
            ));
        }
        let catalogue_game_id: Option<String> = self
            .connection
            .query_row(
                "SELECT id FROM catalogue_games WHERE amethys_id = ?1 AND retired = 0",
                [amethys_id as i64],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| {
                storage_error(format!("Could not check master catalogue game: {error}"))
            })?;
        let Some(catalogue_game_id) = catalogue_game_id else {
            return Ok(false);
        };
        self.connection.execute(
            "INSERT INTO catalogue_external_ids (catalogue_game_id, provider, external_id)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(catalogue_game_id, provider) DO UPDATE SET external_id = excluded.external_id",
            params![catalogue_game_id, provider, external_id],
        ).map(|changed| changed != 0)
            .map_err(|error| storage_error(format!("Could not save master catalogue identity: {error}")))
    }

    /// Reads metadata directly from the canonical Amethys Game ID.
    /// This remains available even when the user has no local installation.
    pub fn get_catalogue_game_metadata(
        &self,
        catalogue_game_id: &str,
    ) -> Result<Option<GameMetadata>, AmethysError> {
        let catalogue_game_id = validate_game_id(catalogue_game_id)?;
        self.connection
            .query_row(
                "SELECT genres_json, developer, publisher, release_date, description,
                    icon_ref, logo_ref, cover_ref, banner_ref
             FROM catalogue_game_metadata WHERE catalogue_game_id = ?1",
                [catalogue_game_id],
                |row| {
                    let genres_json: String = row.get(0)?;
                    let genres = serde_json::from_str(&genres_json).map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })?;
                    Ok(GameMetadata {
                        genres,
                        developer: row.get(1)?,
                        publisher: row.get(2)?,
                        release_date: row.get(3)?,
                        description: row.get(4)?,
                        icon_ref: row.get(5)?,
                        logo_ref: row.get(6)?,
                        cover_ref: row.get(7)?,
                        banner_ref: row.get(8)?,
                    })
                },
            )
            .optional()
            .map_err(|error| {
                storage_error(format!("Could not read catalogue game metadata: {error}"))
            })
    }

    /// Adds or updates one provider ID. A provider ID can identify only one Amethys game.
    pub fn set_external_game_id(
        &self,
        game_id: &str,
        provider: &str,
        external_id: &str,
    ) -> Result<bool, AmethysError> {
        let game_id = validate_game_id(game_id)?;
        let provider = validate_provider_key(provider)?;
        let external_id = external_id.trim();
        if external_id.is_empty()
            || external_id.len() > MAX_PLATFORM_ID_BYTES
            || external_id.contains('\0')
        {
            return Err(storage_error(
                "External game ID is empty, contains NUL, or exceeds the size limit.",
            ));
        }
        let Some(catalogue_game_id) = self.get_catalogue_game_id(game_id)? else {
            return Ok(false);
        };
        let changed = self
            .connection
            .execute(
                "INSERT INTO catalogue_external_ids (catalogue_game_id, provider, external_id)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(catalogue_game_id, provider) DO UPDATE SET external_id = excluded.external_id",
                params![catalogue_game_id, provider, external_id],
            )
            .map_err(|error| storage_error(format!("Could not save external game ID: {error}")))?;
        Ok(changed != 0)
    }

    /// Finds a game by a provider ID, independently of the primary library platform.
    pub fn get_game_by_external_id(
        &self,
        provider: &str,
        external_id: &str,
    ) -> Result<Option<GameEntry>, AmethysError> {
        let provider = validate_provider_key(provider)?;
        let external_id = external_id.trim();
        if external_id.is_empty()
            || external_id.len() > MAX_PLATFORM_ID_BYTES
            || external_id.contains('\0')
        {
            return Err(storage_error("External game ID is invalid."));
        }
        self.connection
            .query_row(
                "SELECT g.id, g.name, g.platform, g.platform_id, g.install_path, g.executable, g.added_at
             FROM catalogue_external_ids e
             INNER JOIN games g ON g.catalogue_game_id = e.catalogue_game_id
             WHERE e.provider = ?1 COLLATE NOCASE AND e.external_id = ?2
             ORDER BY g.added_at, g.id LIMIT 1",
                params![provider, external_id],
                game_from_row,
            )
            .optional()
            .map_err(|error| storage_error(format!("Could not find game by external ID: {error}")))
    }

    /// Returns all provider identities for a game in stable provider order.
    pub fn list_external_game_ids(
        &self,
        game_id: &str,
    ) -> Result<Vec<ExternalGameId>, AmethysError> {
        let game_id = validate_game_id(game_id)?;
        let Some(catalogue_game_id) = self.get_catalogue_game_id(game_id)? else {
            return Ok(Vec::new());
        };
        let mut statement = self
            .connection
            .prepare(
                "SELECT provider, external_id FROM catalogue_external_ids
             WHERE catalogue_game_id = ?1 ORDER BY provider COLLATE NOCASE",
            )
            .map_err(|error| {
                storage_error(format!("Could not prepare external ID list: {error}"))
            })?;
        let rows = statement
            .query_map([catalogue_game_id], |row| {
                Ok(ExternalGameId {
                    provider: row.get(0)?,
                    external_id: row.get(1)?,
                })
            })
            .map_err(|error| storage_error(format!("Could not list external IDs: {error}")))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| storage_error(format!("Could not read external IDs: {error}")))
    }
    /// Returns the canonical Amethys Game ID assigned to a local installation.
    pub fn get_catalogue_game_id(
        &self,
        installation_id: &str,
    ) -> Result<Option<String>, AmethysError> {
        let installation_id = validate_game_id(installation_id)?;
        self.connection
            .query_row(
                "SELECT catalogue_game_id FROM games WHERE id = ?1",
                [installation_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map(|value| value.flatten())
            .map_err(|error| storage_error(format!("Could not read catalogue game ID: {error}")))
    }

    /// Reads one canonical catalogue record.
    pub fn get_catalogue_game(
        &self,
        catalogue_game_id: &str,
    ) -> Result<Option<CatalogueGame>, AmethysError> {
        let catalogue_game_id = validate_game_id(catalogue_game_id)?;
        self.connection
            .query_row(
                "SELECT id, name FROM catalogue_games WHERE id = ?1",
                [catalogue_game_id],
                |row| {
                    Ok(CatalogueGame {
                        id: row.get(0)?,
                        name: row.get(1)?,
                    })
                },
            )
            .optional()
            .map_err(|error| storage_error(format!("Could not read catalogue game: {error}")))
    }

    /// Finds a canonical catalogue record using a provider identity.
    /// Unlike a library lookup, this also works when no installation is present.
    pub fn get_catalogue_game_by_external_id(
        &self,
        provider: &str,
        external_id: &str,
    ) -> Result<Option<CatalogueGame>, AmethysError> {
        let provider = validate_provider_key(provider)?;
        let external_id = external_id.trim();
        if external_id.is_empty()
            || external_id.len() > MAX_PLATFORM_ID_BYTES
            || external_id.contains('\0')
        {
            return Err(storage_error("External game ID is invalid."));
        }
        self.connection
            .query_row(
                "SELECT c.id, c.name
             FROM catalogue_external_ids e
             INNER JOIN catalogue_games c ON c.id = e.catalogue_game_id
             WHERE e.provider = ?1 COLLATE NOCASE AND e.external_id = ?2",
                params![provider, external_id],
                |row| {
                    Ok(CatalogueGame {
                        id: row.get(0)?,
                        name: row.get(1)?,
                    })
                },
            )
            .optional()
            .map_err(|error| {
                storage_error(format!(
                    "Could not find catalogue game by external ID: {error}"
                ))
            })
    }

    /// Links an installation to an existing canonical game without matching by title.
    /// Returns false when either record does not exist; conflicting links are explicit updates.
    pub fn set_game_catalogue_id(
        &self,
        installation_id: &str,
        catalogue_game_id: &str,
    ) -> Result<bool, AmethysError> {
        let installation_id = validate_game_id(installation_id)?;
        let catalogue_game_id = validate_game_id(catalogue_game_id)?;
        let changed = self
            .connection
            .execute(
                "UPDATE games SET catalogue_game_id = ?1
             WHERE id = ?2
               AND EXISTS (SELECT 1 FROM catalogue_games WHERE id = ?1 AND retired = 0)",
                params![catalogue_game_id, installation_id],
            )
            .map_err(|error| {
                storage_error(format!(
                    "Could not link installation to catalogue game: {error}"
                ))
            })?;
        Ok(changed != 0)
    }

    /// Updates the executable configured for an existing game.
    pub fn set_game_executable(
        &self,
        id: &str,
        executable: &str,
    ) -> Result<Option<GameEntry>, AmethysError> {
        let executable = executable.trim();
        let id = validate_game_id(id)?;
        if executable.is_empty() {
            return Err(storage_error("Executable path must not be empty."));
        }
        if executable.contains('\0') {
            return Err(storage_error(
                "Executable paths must not contain NUL characters.",
            ));
        }
        if executable.len() > MAX_GAME_PATH_BYTES {
            return Err(storage_error(format!(
                "Executable path must not exceed {MAX_GAME_PATH_BYTES} bytes."
            )));
        }
        self.connection
            .execute(
                "UPDATE games SET executable = ?1 WHERE id = ?2",
                params![executable, id],
            )
            .map_err(|error| storage_error(format!("Could not update game executable: {error}")))?;
        self.get_game(id)
    }

    /// Refreshes the installation details from a trusted local discovery adapter.
    ///
    /// The stable local library ID and platform identity are preserved. A missing
    /// newly detected executable does not erase a user-configured executable.
    pub fn refresh_detected_installation(
        &self,
        id: &str,
        name: &str,
        install_path: &str,
        executable: Option<&str>,
    ) -> Result<Option<GameEntry>, AmethysError> {
        let id = validate_game_id(id)?;
        let name = name.trim();
        let install_path = install_path.trim();
        let executable = executable.map(str::trim).filter(|value| !value.is_empty());

        if name.is_empty()
            || install_path.is_empty()
            || name.len() > MAX_GAME_NAME_BYTES
            || install_path.len() > MAX_GAME_PATH_BYTES
            || name.contains('\0')
            || install_path.contains('\0')
            || executable
                .is_some_and(|value| value.len() > MAX_GAME_PATH_BYTES || value.contains('\0'))
        {
            return Err(storage_error("Detected installation metadata is invalid."));
        }

        let transaction = self.connection.unchecked_transaction().map_err(|error| {
            storage_error(format!("Could not begin detected installation refresh: {error}"))
        })?;
        let changed = transaction
            .execute(
                "UPDATE games
                 SET name = ?1, install_path = ?2, executable = COALESCE(?3, executable)
                 WHERE id = ?4",
                params![name, install_path, executable, id],
            )
            .map_err(|error| {
                storage_error(format!("Could not refresh detected installation: {error}"))
            })?;
        if changed > 0 {
            sync_primary_game_location(&transaction, id, install_path)?;
        }
        transaction.commit().map_err(|error| {
            storage_error(format!("Could not commit detected installation refresh: {error}"))
        })?;
        self.get_game(id)
    }

    /// Lists all known installation directories, with the primary location first.
    pub fn list_game_locations(&self, game_id: &str) -> Result<Vec<String>, AmethysError> {
        let game_id = validate_game_id(game_id)?;
        let mut statement = self
            .connection
            .prepare(
                "SELECT path FROM local_game_locations
                 WHERE game_id = ?1
                 ORDER BY is_primary DESC, path COLLATE NOCASE, path",
            )
            .map_err(|error| storage_error(format!("Could not prepare game locations: {error}")))?;
        let rows = statement
            .query_map([game_id], |row| row.get(0))
            .map_err(|error| storage_error(format!("Could not query game locations: {error}")))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| storage_error(format!("Could not read game locations: {error}")))
    }

    /// Records another installation directory for an existing library game.
    ///
    /// The current primary installation is never replaced by this operation.
    /// Repeated discovery of the same path is idempotent.
    pub fn add_game_location(&self, game_id: &str, path: &str) -> Result<bool, AmethysError> {
        let game_id = validate_game_id(game_id)?;
        let path = path.trim();
        if path.is_empty() || path.len() > MAX_GAME_PATH_BYTES || path.contains('\0') {
            return Err(storage_error("Game location path is invalid."));
        }

        let transaction = self.connection.unchecked_transaction().map_err(|error| {
            storage_error(format!("Could not begin game location insert: {error}"))
        })?;
        let game_exists: bool = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM games WHERE id = ?1)",
                [game_id],
                |row| row.get(0),
            )
            .map_err(|error| {
                storage_error(format!("Could not check game for location: {error}"))
            })?;
        if !game_exists {
            return Err(storage_error("Cannot add a location to a game that does not exist."));
        }
        let changed = transaction
            .execute(
                "INSERT INTO local_game_locations (id, game_id, path, is_primary)
                 VALUES (?1, ?2, ?3, 0)
                 ON CONFLICT(game_id, path) DO NOTHING",
                params![uuid::Uuid::new_v4().to_string(), game_id, path],
            )
            .map_err(|error| {
                storage_error(format!("Could not add secondary game location: {error}"))
            })?;
        transaction.commit().map_err(|error| {
            storage_error(format!("Could not commit secondary game location: {error}"))
        })?;
        Ok(changed != 0)
    }

    /// Removes a game. Returns false when its identifier was not present.
    pub fn remove_game(&self, id: &str) -> Result<bool, AmethysError> {
        let id = validate_game_id(id)?;
        let changed = self
            .connection
            .execute("DELETE FROM games WHERE id = ?1", [id])
            .map_err(|error| storage_error(format!("Could not remove game: {error}")))?;
        Ok(changed != 0)
    }

    /// Adds or refreshes an owned catalogue game without creating an installation.
    pub fn add_owned_game(
        &self,
        amethys_id: u64,
        name: &str,
        genres: &[String],
    ) -> Result<OwnedGameEntry, AmethysError> {
        let name = name.trim();
        if amethys_id == 0
            || amethys_id > i64::MAX as u64
            || name.is_empty()
            || name.len() > MAX_GAME_NAME_BYTES
            || name.contains('\0')
        {
            return Err(storage_error("Owned game identity or name is invalid."));
        }
        if genres.len() > 64
            || genres
                .iter()
                .any(|genre| genre.trim().is_empty() || genre.len() > 128 || genre.contains('\0'))
        {
            return Err(storage_error("Owned game genres are invalid."));
        }
        let normalized = genres.iter().map(|genre| genre.trim()).collect::<Vec<_>>();
        let genres_json = serde_json::to_string(&normalized).map_err(|error| {
            storage_error(format!("Could not encode owned game genres: {error}"))
        })?;
        self.connection.execute(
            "INSERT INTO owned_games (amethys_id, name, genres_json) VALUES (?1, ?2, ?3)
             ON CONFLICT(amethys_id) DO UPDATE SET name = excluded.name, genres_json = excluded.genres_json",
            params![amethys_id as i64, name, genres_json],
        ).map_err(|error| storage_error(format!("Could not add owned game: {error}")))?;
        self.get_owned_game(amethys_id)?
            .ok_or_else(|| storage_error("Added owned game could not be read back."))
    }

    pub fn get_owned_game(&self, amethys_id: u64) -> Result<Option<OwnedGameEntry>, AmethysError> {
        if amethys_id == 0 || amethys_id > i64::MAX as u64 {
            return Err(storage_error("Owned game ID is invalid."));
        }
        self.connection.query_row(
            "SELECT amethys_id, name, genres_json, added_at FROM owned_games WHERE amethys_id = ?1",
            [amethys_id as i64],
            |row| {
                let genres_json: String = row.get(2)?;
                let genres = serde_json::from_str(&genres_json).map_err(|error| rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(error)))?;
                let id: i64 = row.get(0)?;
                Ok(OwnedGameEntry { amethys_id: id as u64, name: row.get(1)?, genres, added_at: row.get(3)? })
            },
        ).optional().map_err(|error| storage_error(format!("Could not read owned game: {error}")))
    }

    pub fn list_owned_games(&self) -> Result<Vec<OwnedGameEntry>, AmethysError> {
        let mut statement = self.connection.prepare("SELECT amethys_id, name, genres_json, added_at FROM owned_games ORDER BY name COLLATE NOCASE, amethys_id")
            .map_err(|error| storage_error(format!("Could not prepare owned-game list: {error}")))?;
        let rows = statement
            .query_map([], |row| {
                let genres_json: String = row.get(2)?;
                let genres = serde_json::from_str(&genres_json).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        2,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?;
                let id: i64 = row.get(0)?;
                Ok(OwnedGameEntry {
                    amethys_id: id as u64,
                    name: row.get(1)?,
                    genres,
                    added_at: row.get(3)?,
                })
            })
            .map_err(|error| storage_error(format!("Could not list owned games: {error}")))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| storage_error(format!("Could not read owned games: {error}")))
    }

    pub fn remove_owned_game(&self, amethys_id: u64) -> Result<bool, AmethysError> {
        if amethys_id == 0 || amethys_id > i64::MAX as u64 {
            return Err(storage_error("Owned game ID is invalid."));
        }
        self.connection
            .execute(
                "DELETE FROM owned_games WHERE amethys_id = ?1",
                [amethys_id as i64],
            )
            .map(|changed| changed != 0)
            .map_err(|error| storage_error(format!("Could not remove owned game: {error}")))
    }

    /// Reads one application setting without changing the database.
    pub fn get_setting(&self, key: &str) -> Result<Option<String>, AmethysError> {
        let key = validate_setting_key(key)?;
        self.connection
            .query_row(
                "SELECT value FROM app_settings WHERE key = ?1",
                [key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| storage_error(format!("Could not read setting: {error}")))
    }

    /// Inserts or replaces one application setting atomically.
    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), AmethysError> {
        let key = validate_setting_key(key)?;
        if value.contains('\0') {
            return Err(storage_error(
                "Setting values must not contain NUL characters.",
            ));
        }
        const MAX_SETTING_VALUE_BYTES: usize = 64 * 1024;
        if value.len() > MAX_SETTING_VALUE_BYTES {
            return Err(storage_error(format!(
                "Setting value exceeds the {MAX_SETTING_VALUE_BYTES}-byte limit."
            )));
        }
        self.connection
            .execute(
                "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map_err(|error| storage_error(format!("Could not save setting: {error}")))?;
        Ok(())
    }

    /// Removes one setting. Returns false when the key was not present.
    pub fn remove_setting(&self, key: &str) -> Result<bool, AmethysError> {
        let key = validate_setting_key(key)?;
        let changed = self
            .connection
            .execute("DELETE FROM app_settings WHERE key = ?1", [key])
            .map_err(|error| storage_error(format!("Could not remove setting: {error}")))?;
        Ok(changed != 0)
    }

    /// Returns all settings in deterministic key order.
    pub fn list_settings(&self) -> Result<Vec<(String, String)>, AmethysError> {
        let mut statement = self
            .connection
            .prepare("SELECT key, value FROM app_settings ORDER BY key COLLATE NOCASE, key")
            .map_err(|error| storage_error(format!("Could not prepare settings list: {error}")))?;
        let rows = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|error| storage_error(format!("Could not list settings: {error}")))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| storage_error(format!("Could not read settings: {error}")))
    }

    #[must_use]
    pub const fn connection(&self) -> &Connection {
        &self.connection
    }
}


/// Keeps the normalized primary-location row in step with the legacy game path.
///
/// Secondary locations are retained; only the previous primary row is replaced.
/// The caller runs this inside the same transaction as the corresponding game write.
fn sync_primary_game_location(
    connection: &Connection,
    game_id: &str,
    install_path: &str,
) -> Result<(), AmethysError> {
    connection
        .execute(
            "DELETE FROM local_game_locations
             WHERE game_id = ?1 AND is_primary = 1 AND path <> ?2",
            params![game_id, install_path],
        )
        .map_err(|error| storage_error(format!("Could not replace primary game location: {error}")))?;
    connection
        .execute(
            "INSERT INTO local_game_locations (id, game_id, path, is_primary)
             VALUES (?1, ?2, ?3, 1)
             ON CONFLICT(game_id, path) DO UPDATE SET is_primary = 1",
            params![uuid::Uuid::new_v4().to_string(), game_id, install_path],
        )
        .map_err(|error| storage_error(format!("Could not save primary game location: {error}")))?;
    Ok(())
}

fn game_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<GameEntry> {
    Ok(GameEntry {
        id: row.get(0)?,
        name: row.get(1)?,
        platform: row.get(2)?,
        platform_id: row.get(3)?,
        install_path: row.get(4)?,
        executable: row.get(5)?,
        added_at: row.get(6)?,
    })
}

fn validate_game_id(id: &str) -> Result<&str, AmethysError> {
    let id = id.trim();
    if id.is_empty() {
        return Err(storage_error("Game IDs must not be empty."));
    }
    if id.contains('\0') {
        return Err(storage_error("Game IDs must not contain NUL characters."));
    }
    if id.len() > MAX_GAME_ID_BYTES {
        return Err(storage_error(format!(
            "Game ID exceeds the {MAX_GAME_ID_BYTES}-byte limit."
        )));
    }
    Ok(id)
}

fn validate_platform_identity<'a>(
    platform: &'a str,
    platform_id: &'a str,
) -> Result<(&'a str, &'a str), AmethysError> {
    let platform = platform.trim();
    let platform_id = platform_id.trim();
    if platform.is_empty() || platform_id.is_empty() {
        return Err(storage_error(
            "Platform name and platform game ID must not be empty.",
        ));
    }
    if platform.contains('\0') || platform_id.contains('\0') {
        return Err(storage_error(
            "Platform identity must not contain NUL characters.",
        ));
    }
    if platform.len() > MAX_PLATFORM_NAME_BYTES {
        return Err(storage_error(format!(
            "Platform name exceeds the {MAX_PLATFORM_NAME_BYTES}-byte limit."
        )));
    }
    if platform_id.len() > MAX_PLATFORM_ID_BYTES {
        return Err(storage_error(format!(
            "Platform game ID exceeds the {MAX_PLATFORM_ID_BYTES}-byte limit."
        )));
    }
    Ok((platform, platform_id))
}

fn validate_provider_key(provider: &str) -> Result<&str, AmethysError> {
    let provider = provider.trim();
    if provider.is_empty()
        || provider.len() > 48
        || !provider
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(storage_error(
            "Provider keys must use lowercase ASCII letters, digits or hyphens.",
        ));
    }
    Ok(provider)
}

fn validate_setting_key(key: &str) -> Result<&str, AmethysError> {
    let key = key.trim();
    const MAX_SETTING_KEY_BYTES: usize = 256;
    if key.is_empty() {
        return Err(storage_error("Setting keys must not be empty."));
    }
    if key.contains('\0') {
        return Err(storage_error(
            "Setting keys must not contain NUL characters.",
        ));
    }
    if key.len() > MAX_SETTING_KEY_BYTES {
        return Err(storage_error(format!(
            "Setting key exceeds the {MAX_SETTING_KEY_BYTES}-byte limit."
        )));
    }
    Ok(key)
}

fn storage_error(message: impl Into<String>) -> AmethysError {
    AmethysError::new(ErrorCode::StorageQuery, message)
}

#[cfg(test)]
mod tests {
    use super::{CatalogueGame, Database};

    fn temporary_database() -> (Database, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("amethys-{}.sqlite", uuid::Uuid::new_v4()));
        let database = Database::open(&path).expect("database opens");
        (database, path)
    }

    #[test]
    fn owned_games_are_independent_from_installations() {
        let database = Database::open(":memory:").expect("database opens");
        let owned = database
            .add_owned_game(
                9001,
                "Owned without install",
                &["RPG".to_owned(), "Adventure".to_owned()],
            )
            .unwrap();
        assert_eq!(owned.amethys_id, 9001);
        assert!(database.list_games().unwrap().is_empty());
        assert_eq!(database.list_owned_games().unwrap().len(), 1);
        assert!(database.remove_owned_game(9001).unwrap());
        assert!(database.list_owned_games().unwrap().is_empty());
    }

    #[test]
    fn deleting_an_installation_preserves_its_master_catalogue_record() {
        let database = Database::open(":memory:").expect("database opens");
        database
            .register_catalogue_game("7000", 7000, "Master Game", false)
            .unwrap();
        let game = database
            .add_game("Master Game", "Steam", "/games/master", None)
            .unwrap();
        assert!(database.set_game_catalogue_id(&game.id, "7000").unwrap());
        database
            .set_catalogue_external_id(7000, "steam", "98765")
            .unwrap();
        database
            .set_game_metadata(
                &game.id,
                &super::GameMetadata {
                    developer: Some("Local note".to_owned()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(database.remove_game(&game.id).unwrap());
        assert_eq!(database.get_game_metadata(&game.id).unwrap(), None);
        assert_eq!(
            database.get_catalogue_game("7000").unwrap().unwrap().name,
            "Master Game"
        );
        assert_eq!(
            database
                .get_catalogue_game_by_external_id("steam", "98765")
                .unwrap()
                .unwrap()
                .id,
            "7000"
        );
    }

    #[test]
    fn local_imports_never_allocate_master_catalogue_ids() {
        let database = Database::open(":memory:").expect("database opens");
        let first = database
            .add_game("First", "Local", "/games/first", None)
            .unwrap();
        let second = database
            .add_game("Second", "Local", "/games/second", None)
            .unwrap();
        assert_eq!(database.get_catalogue_game_id(&first.id).unwrap(), None);
        assert_eq!(database.get_catalogue_game_id(&second.id).unwrap(), None);
        let count: i64 = database
            .connection
            .query_row("SELECT COUNT(*) FROM catalogue_games", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn local_metadata_and_verified_master_store_ids_are_separate() {
        let database = Database::open(":memory:").expect("database opens");
        database
            .register_catalogue_game("8123", 8123, "Example", false)
            .unwrap();
        let game = database
            .add_game("Example", "Steam", "/games/example", None)
            .unwrap();
        assert!(database.set_game_catalogue_id(&game.id, "8123").unwrap());
        let metadata = super::GameMetadata {
            genres: vec!["Action".to_owned(), "Adventure".to_owned()],
            developer: Some("Studio".to_owned()),
            publisher: Some("Publisher".to_owned()),
            release_date: Some("2026-11-22".to_owned()),
            description: Some("Local installation metadata.".to_owned()),
            icon_ref: Some("media/library/icon.png".to_owned()),
            logo_ref: None,
            cover_ref: Some("media/library/cover.png".to_owned()),
            banner_ref: None,
        };
        assert!(database.set_game_metadata(&game.id, &metadata).unwrap());
        assert_eq!(
            database.get_game_metadata(&game.id).unwrap(),
            Some(metadata)
        );
        assert!(
            database
                .set_catalogue_external_id(8123, "steam", "123456")
                .unwrap()
        );
        assert!(
            database
                .set_catalogue_external_id(8123, "epic", "epic-product-abc")
                .unwrap()
        );
        assert_eq!(
            database
                .get_game_by_external_id("steam", "123456")
                .unwrap()
                .unwrap()
                .id,
            game.id
        );
        assert_eq!(database.list_external_game_ids(&game.id).unwrap().len(), 2);
    }

    #[test]
    fn master_external_provider_ids_cannot_be_ambiguous() {
        let database = Database::open(":memory:").expect("database opens");
        database
            .register_catalogue_game("game-100", 100, "First", false)
            .unwrap();
        database
            .register_catalogue_game("game-101", 101, "Second", false)
            .unwrap();
        database
            .set_catalogue_external_id(100, "steam", "42")
            .unwrap();
        assert!(
            database
                .set_catalogue_external_id(101, "steam", "42")
                .is_err()
        );
        assert!(
            database
                .set_catalogue_external_id(100, "../steam", "99")
                .is_err()
        );
        assert!(
            database
                .set_catalogue_external_id(100, "steam", "000099")
                .unwrap()
        );
    }

    #[test]
    fn metadata_requires_an_existing_game_and_rejects_invalid_fields() {
        let database = Database::open(":memory:").expect("database opens");
        assert!(
            !database
                .set_game_metadata("missing", &super::GameMetadata::default())
                .expect("missing game is ignored")
        );
        let game = database
            .add_game("Valid", "Local", "/games/valid", None)
            .expect("game added");
        let invalid = super::GameMetadata {
            genres: vec!["\0bad".to_owned()],
            ..Default::default()
        };
        assert!(database.set_game_metadata(&game.id, &invalid).is_err());
        let invalid = super::GameMetadata {
            developer: Some("\0bad".to_owned()),
            ..Default::default()
        };
        assert!(database.set_game_metadata(&game.id, &invalid).is_err());
        assert!(
            !database
                .set_external_game_id("missing", "steam", "123")
                .expect("missing game ignored")
        );
    }
    #[test]
    fn adding_game_creates_a_matching_primary_location_atomically() {
        let database = Database::open(":memory:").expect("database opens");
        let game = database
            .add_game("Example", "Steam", "/games/example", None)
            .expect("game is added");

        let locations: Vec<(String, i64)> = database
            .connection()
            .prepare(
                "SELECT path, is_primary FROM local_game_locations
                 WHERE game_id = ?1 ORDER BY path",
            )
            .expect("location query prepares")
            .query_map([&game.id], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("location query runs")
            .collect::<Result<_, _>>()
            .expect("locations are readable");
        assert_eq!(locations, vec![("/games/example".to_owned(), 1)]);
    }

    #[test]
    fn refreshing_game_replaces_only_the_primary_location() {
        let database = Database::open(":memory:").expect("database opens");
        let game = database
            .add_game("Example", "Steam", "/games/old", None)
            .expect("game is added");
        database
            .connection()
            .execute(
                "INSERT INTO local_game_locations (id, game_id, path, is_primary)
                 VALUES ('secondary-location', ?1, '/games/secondary', 0)",
                [&game.id],
            )
            .expect("secondary location is inserted");

        let refreshed = database
            .refresh_detected_installation(&game.id, "Example", "/games/new", None)
            .expect("refresh succeeds")
            .expect("game remains available");
        assert_eq!(refreshed.install_path, "/games/new");

        let locations: Vec<(String, i64)> = database
            .connection()
            .prepare(
                "SELECT path, is_primary FROM local_game_locations
                 WHERE game_id = ?1 ORDER BY path",
            )
            .expect("location query prepares")
            .query_map([&game.id], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("location query runs")
            .collect::<Result<_, _>>()
            .expect("locations are readable");
        assert_eq!(
            locations,
            vec![
                ("/games/new".to_owned(), 1),
                ("/games/secondary".to_owned(), 0)
            ]
        );
    }

    #[test]
    fn adding_secondary_location_is_idempotent_and_keeps_primary() {
        let database = Database::open(":memory:").expect("database opens");
        let game = database
            .add_game("Example", "Steam", "/games/primary", None)
            .expect("game is added");

        assert!(database
            .add_game_location(&game.id, "/games/secondary")
            .expect("secondary location is inserted"));
        assert!(!database
            .add_game_location(&game.id, "/games/secondary")
            .expect("duplicate location is ignored"));

        let locations: Vec<(String, i64)> = database
            .connection()
            .prepare(
                "SELECT path, is_primary FROM local_game_locations
                 WHERE game_id = ?1 ORDER BY path",
            )
            .expect("location query prepares")
            .query_map([&game.id], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("location query runs")
            .collect::<Result<_, _>>()
            .expect("locations are readable");
        assert_eq!(
            locations,
            vec![
                ("/games/primary".to_owned(), 1),
                ("/games/secondary".to_owned(), 0)
            ]
        );
    }

    #[test]
    fn refreshing_detected_installation_updates_moved_path_without_changing_library_id() {
        let database = Database::open(":memory:").expect("database opens");
        let original = database
            .add_game_with_platform_id(
                "Old title",
                "Steam",
                Some("570"),
                "/games/old-location",
                Some("/games/old-location/game.exe"),
            )
            .expect("initial installation is added");

        let refreshed = database
            .refresh_detected_installation(
                &original.id,
                "Dota 2",
                "/games/new-location",
                Some("/games/new-location/game.exe"),
            )
            .expect("detected installation refresh succeeds")
            .expect("installation remains present");

        assert_eq!(refreshed.id, original.id);
        assert_eq!(refreshed.platform_id.as_deref(), Some("570"));
        assert_eq!(refreshed.name, "Dota 2");
        assert_eq!(refreshed.install_path, "/games/new-location");
        assert_eq!(
            refreshed.executable.as_deref(),
            Some("/games/new-location/game.exe")
        );
        assert_eq!(database.list_games().expect("list games").len(), 1);
    }

    #[test]
    fn refreshing_detected_installation_preserves_manual_executable_when_discovery_has_none() {
        let database = Database::open(":memory:").expect("database opens");
        let original = database
            .add_game_with_platform_id(
                "Example",
                "Steam",
                Some("123"),
                "/games/old",
                Some("custom-launcher.exe"),
            )
            .expect("initial installation is added");

        let refreshed = database
            .refresh_detected_installation(&original.id, "Example renamed", "/games/new", None)
            .expect("refresh succeeds")
            .expect("installation remains present");

        assert_eq!(refreshed.install_path, "/games/new");
        assert_eq!(refreshed.executable.as_deref(), Some("custom-launcher.exe"));
    }

    #[test]
    fn legacy_game_can_be_upgraded_with_platform_identity_without_changing_library_id() {
        let database = Database::open(":memory:").expect("database opens");
        let legacy = database
            .add_game("Legacy game", "Steam", "/games/legacy", None)
            .expect("legacy game is added");

        let upgraded = database
            .set_game_platform_id(&legacy.id, "steam", "12345")
            .expect("platform ID is attached")
            .expect("game remains present");

        assert_eq!(upgraded.id, legacy.id);
        assert_eq!(upgraded.platform_id.as_deref(), Some("12345"));
        assert_eq!(
            database
                .get_game_by_platform_id("Steam", "12345")
                .expect("platform identity lookup succeeds")
                .expect("upgraded game is found")
                .id,
            legacy.id
        );
    }

    #[test]
    fn legacy_platform_identity_upgrade_rejects_conflicting_existing_identity() {
        let database = Database::open(":memory:").expect("database opens");
        let legacy = database
            .add_game("Legacy game", "Steam", "/games/legacy", None)
            .expect("legacy game is added");
        let upgraded = database
            .set_game_platform_id(&legacy.id, "Steam", "12345")
            .expect("first identity assignment succeeds")
            .expect("legacy entry is upgraded");

        assert!(
            database
                .set_game_platform_id(&legacy.id, "Steam", "67890")
                .expect("conflicting assignment is handled")
                .is_none(),
            "a different existing platform ID must not be reported as assigned"
        );
        assert!(
            database
                .set_game_platform_id(&legacy.id, "Epic", "12345")
                .expect("wrong-platform assignment is handled")
                .is_none(),
            "an identity from another platform must not be accepted"
        );

        let stored = database
            .get_game(&legacy.id)
            .expect("game lookup succeeds")
            .expect("game remains present");
        assert_eq!(stored.id, upgraded.id);
        assert_eq!(stored.platform, "Steam");
        assert_eq!(stored.platform_id.as_deref(), Some("12345"));
    }

    #[test]
    fn database_opens_from_a_relative_filename() {
        let filename = format!("amethys-{}.sqlite", uuid::Uuid::new_v4());
        let path = std::path::Path::new(&filename);
        let database = Database::open(path).expect("relative database path opens");
        let count: i64 = database
            .connection()
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .expect("migration table readable");
        assert_eq!(count, 7);
        drop(database);
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(format!("{filename}-wal"));
        let _ = std::fs::remove_file(format!("{filename}-shm"));
    }

    #[test]
    fn failed_schema_migration_rolls_back_partial_tables() {
        let path = std::env::temp_dir().join(format!("amethys-{}.sqlite", uuid::Uuid::new_v4()));
        {
            let connection = rusqlite::Connection::open(&path).expect("fixture database opens");
            connection
                .execute_batch("CREATE TABLE schema_migrations (wrong TEXT);")
                .expect("incompatible migration table is created");
        }

        let error = match Database::open(&path) {
            Err(error) => error,
            Ok(_) => panic!("incompatible schema must fail"),
        };
        assert_eq!(error.code(), amethys_errors::ErrorCode::StorageMigration);

        let connection = rusqlite::Connection::open(&path).expect("fixture database reopens");
        let app_settings_exists: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'app_settings'",
                [],
                |row| row.get(0),
            )
            .expect("schema catalog is readable");
        assert_eq!(
            app_settings_exists, 0,
            "partial table creation must be rolled back"
        );
        drop(connection);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }

    #[test]
    fn newer_database_schema_is_rejected_without_modification() {
        let path = std::env::temp_dir().join(format!("amethys-{}.sqlite", uuid::Uuid::new_v4()));
        {
            let connection = rusqlite::Connection::open(&path).expect("fixture database opens");
            connection
                .execute_batch(
                    "CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY NOT NULL,
                    applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );
                INSERT INTO schema_migrations (version) VALUES (8);",
                )
                .expect("future schema is created");
        }

        let error = match Database::open(&path) {
            Err(error) => error,
            Ok(_) => panic!("newer schema must be rejected"),
        };
        assert_eq!(error.code(), amethys_errors::ErrorCode::StorageMigration);
        assert!(
            error
                .message()
                .contains("newer than the supported version 7")
        );

        let connection = rusqlite::Connection::open(&path).expect("fixture database reopens");
        let games_exists: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'games'",
                [],
                |row| row.get(0),
            )
            .expect("schema catalog is readable");
        assert_eq!(games_exists, 0, "future database must remain untouched");
        let journal_mode: String = connection
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .expect("journal mode is readable");
        assert_eq!(
            journal_mode.to_ascii_lowercase(),
            "delete",
            "rejecting a future schema must not change its persistent journal mode"
        );
        drop(connection);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }

    #[test]
    fn version_two_database_migrates_without_losing_existing_games() {
        let path = std::env::temp_dir().join(format!("amethys-v2-{}.sqlite", uuid::Uuid::new_v4()));
        {
            let connection = rusqlite::Connection::open(&path).expect("fixture database opens");
            connection
                .execute_batch(
                    "CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY NOT NULL,
                    applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );
                INSERT INTO schema_migrations (version) VALUES (1), (2);
                CREATE TABLE games (
                    id TEXT PRIMARY KEY NOT NULL,
                    name TEXT NOT NULL,
                    platform TEXT NOT NULL,
                    install_path TEXT NOT NULL,
                    executable TEXT,
                    added_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );
                INSERT INTO games (id, name, platform, install_path)
                VALUES ('legacy-id', 'Legacy Game', 'Steam', '/games/legacy');",
                )
                .expect("version two fixture is created");
        }

        let database = Database::open(&path).expect("version two database migrates");
        let game = database
            .get_game("legacy-id")
            .expect("legacy lookup works")
            .expect("legacy game remains");
        assert_eq!(game.name, "Legacy Game");
        assert_eq!(game.platform_id, None);
        assert_eq!(
            database
                .get_catalogue_game_id(&game.id)
                .expect("legacy catalogue ID lookup")
                .as_deref(),
            None
        );
        let version: i64 = database
            .connection()
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .expect("version is readable");
        assert_eq!(version, 7);
        drop(database);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }

    #[test]
    fn version_one_database_migrates_and_preserves_existing_settings() {
        let path = std::env::temp_dir().join(format!("amethys-v1-{}.sqlite", uuid::Uuid::new_v4()));
        {
            let connection = rusqlite::Connection::open(&path).expect("fixture database opens");
            connection
                .execute_batch(
                    "CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY NOT NULL,
                    applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );
                INSERT INTO schema_migrations (version) VALUES (1);
                CREATE TABLE app_settings (
                    key TEXT PRIMARY KEY NOT NULL,
                    value TEXT NOT NULL
                );
                INSERT INTO app_settings (key, value)
                VALUES ('theme', 'dark');",
                )
                .expect("version one fixture is created");
        }

        let database = Database::open(&path).expect("version one database migrates");
        let setting: String = database
            .connection()
            .query_row(
                "SELECT value FROM app_settings WHERE key = 'theme'",
                [],
                |row| row.get(0),
            )
            .expect("existing setting remains readable");
        assert_eq!(setting, "dark");

        let version: i64 = database
            .connection()
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .expect("migration version is readable");
        assert_eq!(version, 7);
        assert!(
            database
                .list_games()
                .expect("new game table is readable")
                .is_empty()
        );
        drop(database);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }

    #[test]
    fn game_metadata_rejects_embedded_nul_characters() {
        let (database, path) = temporary_database();
        assert!(
            database
                .add_game("Bad\0Name", "Steam", "/games/example", None)
                .is_err()
        );
        assert!(
            database
                .add_game("Example", "Steam", "/games/\0example", None)
                .is_err()
        );
        assert!(
            database
                .add_game("Example", "Steam", "/games/example", Some("game\0.exe"))
                .is_err()
        );
        assert!(
            database
                .add_game_with_platform_id("Example", "Steam", Some("57\0"), "/games/example", None)
                .is_err()
        );
        assert!(
            database
                .list_games()
                .expect("list remains readable")
                .is_empty()
        );
        drop(database);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }

    #[test]
    fn game_metadata_enforces_field_size_limits() {
        let (database, path) = temporary_database();
        let oversized_name = "n".repeat(super::MAX_GAME_NAME_BYTES + 1);
        let oversized_platform = "p".repeat(super::MAX_PLATFORM_NAME_BYTES + 1);
        let oversized_id = "i".repeat(super::MAX_PLATFORM_ID_BYTES + 1);
        let oversized_path = format!("/games/{}", "x".repeat(super::MAX_GAME_PATH_BYTES));

        assert!(
            database
                .add_game(&oversized_name, "Steam", "/games/example", None)
                .is_err()
        );
        assert!(
            database
                .add_game("Example", &oversized_platform, "/games/example", None)
                .is_err()
        );
        assert!(
            database
                .add_game_with_platform_id(
                    "Example",
                    "Steam",
                    Some(&oversized_id),
                    "/games/example",
                    None
                )
                .is_err()
        );
        assert!(
            database
                .add_game("Example", "Steam", &oversized_path, None)
                .is_err()
        );
        assert!(
            database
                .add_game("Example", "Steam", "/games/example", Some(&oversized_path))
                .is_err()
        );
        assert!(
            database
                .list_games()
                .expect("list remains readable")
                .is_empty()
        );
        drop(database);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }

    #[test]
    fn optional_executable_is_trimmed_and_empty_values_become_none() {
        let (database, path) = temporary_database();
        let game = database
            .add_game(
                " Example ",
                " Steam ",
                "/games/example",
                Some("  bin/game.exe  "),
            )
            .expect("valid game is added");
        assert_eq!(game.name, "Example");
        assert_eq!(game.platform, "Steam");
        assert_eq!(game.executable.as_deref(), Some("bin/game.exe"));

        let without_executable = database
            .add_game("Another", "Steam", "/games/another", Some("   "))
            .expect("blank optional executable is accepted");
        assert_eq!(without_executable.executable, None);
        drop(database);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }

    #[test]
    fn platform_ids_are_saved_and_unique_per_platform() {
        let (database, path) = temporary_database();
        let game = database
            .add_game_with_platform_id("Example Game", "Steam", Some("570"), "/games/example", None)
            .expect("game with platform ID is added");
        assert_eq!(game.platform_id.as_deref(), Some("570"));
        assert_eq!(
            database
                .get_game_by_platform_id("steam", "570")
                .expect("lookup works"),
            Some(game)
        );
        assert!(
            database
                .add_game_with_platform_id(
                    "Duplicate",
                    "Steam",
                    Some("570"),
                    "/games/duplicate",
                    None
                )
                .is_err()
        );
        assert!(
            database
                .add_game_with_platform_id(
                    "Duplicate with different casing",
                    "steam",
                    Some("570"),
                    "/games/duplicate-case",
                    None
                )
                .is_err()
        );
        // The same identifier may exist on a different platform.
        assert!(
            database
                .add_game_with_platform_id(
                    "Other Platform Game",
                    "Epic Games",
                    Some("570"),
                    "/games/other",
                    None
                )
                .is_ok()
        );
        drop(database);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }

    #[test]
    fn platform_identity_lookup_rejects_invalid_input() {
        let (database, path) = temporary_database();
        let game = database
            .add_game_with_platform_id("Example", "Steam", Some("570"), "/games/example", None)
            .expect("game is added");

        assert_eq!(
            database
                .get_game_by_platform_id(" Steam ", " 570 ")
                .expect("trimmed identity lookup succeeds"),
            Some(game)
        );
        assert!(database.get_game_by_platform_id("", "570").is_err());
        assert!(database.get_game_by_platform_id("Steam", " ").is_err());
        assert!(
            database
                .get_game_by_platform_id("bad\0platform", "570")
                .is_err()
        );
        assert!(
            database
                .get_game_by_platform_id("Steam", "bad\0id")
                .is_err()
        );
        assert!(
            database
                .get_game_by_platform_id(&"p".repeat(super::MAX_PLATFORM_NAME_BYTES + 1), "570")
                .is_err()
        );
        assert!(
            database
                .get_game_by_platform_id("Steam", &"i".repeat(super::MAX_PLATFORM_ID_BYTES + 1))
                .is_err()
        );

        drop(database);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }

    #[test]
    fn database_uses_full_synchronous_commits_for_durability() {
        let (database, path) = temporary_database();
        let synchronous: i64 = database
            .connection()
            .query_row("PRAGMA synchronous", [], |row| row.get(0))
            .expect("synchronous mode is readable");
        assert_eq!(synchronous, 2, "FULL synchronous mode must be enabled");
        drop(database);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }

    #[test]
    fn database_configures_a_busy_timeout_for_lock_contention() {
        let (database, path) = temporary_database();
        let timeout_ms: i64 = database
            .connection()
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
            .expect("busy timeout is readable");
        assert_eq!(timeout_ms, 5_000);
        drop(database);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }

    #[test]
    fn database_initializes_schema() {
        let (database, path) = temporary_database();
        let count: i64 = database
            .connection()
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .expect("migration table readable");
        assert_eq!(count, 7);
        drop(database);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn game_identifiers_are_trimmed_and_bounded_at_the_storage_boundary() {
        let (database, path) = temporary_database();
        let game = database
            .add_game("Example", "Steam", "/games/example", None)
            .expect("game is added");

        assert_eq!(
            database
                .get_game(&format!(" {} ", game.id))
                .expect("trimmed lookup succeeds"),
            Some(game.clone())
        );
        assert!(
            database
                .set_game_executable(&format!(" {} ", game.id), "bin/game")
                .expect("trimmed update succeeds")
                .is_some()
        );
        assert!(
            database.get_game("").is_err(),
            "empty identifiers must be rejected"
        );
        assert!(database.get_game("bad\0id").is_err());
        assert!(
            database
                .get_game(&"x".repeat(super::MAX_GAME_ID_BYTES + 1))
                .is_err()
        );
        assert!(database.remove_game(" ").is_err());
        assert!(database.remove_game("bad\0id").is_err());
        assert!(
            database
                .remove_game(&"x".repeat(super::MAX_GAME_ID_BYTES + 1))
                .is_err()
        );

        drop(database);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }

    #[test]
    fn imported_games_can_be_configured_with_an_executable() {
        let (database, path) = temporary_database();
        let game = database
            .add_game("Example", "Steam", "/games/example", None)
            .expect("game is added");
        let updated = database
            .set_game_executable(&game.id, "bin/game")
            .expect("executable is configured")
            .expect("game exists");
        assert_eq!(updated.executable.as_deref(), Some("bin/game"));
        assert!(
            database
                .set_game_executable("missing-id", "bin/game")
                .unwrap()
                .is_none()
        );
        assert!(database.set_game_executable(&game.id, " ").is_err());
        assert!(database.set_game_executable(&game.id, "bad\0path").is_err());
        drop(database);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }

    #[test]
    fn games_can_be_added_listed_found_and_removed() {
        let (database, path) = temporary_database();
        let game = database
            .add_game(
                "  Example Game  ",
                "Steam",
                "/games/example",
                Some("game.exe"),
            )
            .expect("game is added");
        assert_eq!(game.name, "Example Game");
        assert_eq!(
            database.get_game(&game.id).expect("lookup works"),
            Some(game.clone())
        );
        assert_eq!(
            database.list_games().expect("list works"),
            vec![game.clone()]
        );
        assert!(database.remove_game(&game.id).expect("remove works"));
        assert!(!database.remove_game(&game.id).expect("second remove works"));
        assert!(database.get_game(&game.id).expect("lookup works").is_none());
        drop(database);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn games_reject_missing_required_fields() {
        let (database, path) = temporary_database();
        assert!(
            database
                .add_game(" ", "Steam", "/games/example", None)
                .is_err()
        );
        assert!(
            database
                .add_game("Example", "", "/games/example", None)
                .is_err()
        );
        assert!(database.add_game("Example", "Steam", " ", None).is_err());
        drop(database);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn application_settings_can_be_created_updated_listed_and_removed() {
        let (database, path) = temporary_database();
        assert_eq!(database.get_setting("theme").expect("lookup works"), None);
        database
            .set_setting(" theme ", "dark")
            .expect("setting is inserted");
        assert_eq!(
            database
                .get_setting("theme")
                .expect("lookup works")
                .as_deref(),
            Some("dark")
        );
        database
            .set_setting("theme", "light")
            .expect("setting is updated");
        database
            .set_setting("language", "fr-FR")
            .expect("second setting is inserted");
        assert_eq!(
            database.list_settings().expect("settings list works"),
            vec![
                ("language".to_owned(), "fr-FR".to_owned()),
                ("theme".to_owned(), "light".to_owned()),
            ]
        );
        assert!(
            database
                .remove_setting("theme")
                .expect("setting is removed")
        );
        assert!(
            !database
                .remove_setting("theme")
                .expect("missing setting is ignored")
        );
        assert_eq!(database.get_setting("theme").expect("lookup works"), None);
        drop(database);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }

    #[test]
    fn application_settings_reject_invalid_keys_and_values() {
        let (database, path) = temporary_database();
        assert!(database.set_setting("", "value").is_err());
        assert!(database.set_setting("   ", "value").is_err());
        assert!(database.set_setting("bad\0key", "value").is_err());
        assert!(database.set_setting("valid", "bad\0value").is_err());
        assert!(database.get_setting("bad\0key").is_err());
        assert!(database.remove_setting("bad\0key").is_err());
        assert!(database.set_setting(&"k".repeat(257), "value").is_err());
        assert!(
            database
                .set_setting("valid", &"v".repeat(64 * 1024 + 1))
                .is_err()
        );
        assert!(
            database
                .list_settings()
                .expect("settings remain readable")
                .is_empty()
        );
        drop(database);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }

    #[test]
    fn application_settings_persist_after_reopening_database() {
        let path =
            std::env::temp_dir().join(format!("amethys-settings-{}.sqlite", uuid::Uuid::new_v4()));
        {
            let database = Database::open(&path).expect("database opens");
            database
                .set_setting("startup.auto_start", "true")
                .expect("setting is saved");
        }
        let database = Database::open(&path).expect("database reopens");
        assert_eq!(
            database
                .get_setting("startup.auto_start")
                .expect("setting is readable")
                .as_deref(),
            Some("true")
        );
        drop(database);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }

    #[test]
    fn concurrent_initial_opens_apply_schema_migrations_once() {
        let path = std::env::temp_dir().join(format!(
            "amethys-concurrent-migration-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));

        let first_path = path.clone();
        let first_barrier = std::sync::Arc::clone(&barrier);
        let first = std::thread::spawn(move || {
            first_barrier.wait();
            Database::open(first_path).expect("first concurrent open succeeds")
        });

        let second_path = path.clone();
        let second_barrier = std::sync::Arc::clone(&barrier);
        let second = std::thread::spawn(move || {
            second_barrier.wait();
            Database::open(second_path).expect("second concurrent open succeeds")
        });

        barrier.wait();
        let first = first.join().expect("first opener thread completes");
        let second = second.join().expect("second opener thread completes");

        // SQLite stores the journal mode in the database header: value 2 at
        // offsets 18 and 19 means WAL. Checking the header avoids relying on
        // rusqlite's PRAGMA query APIs, which cannot read journal_mode here.
        let header = std::fs::read(&path).expect("database header is readable");
        assert!(header.len() >= 100, "SQLite database header is complete");
        assert_eq!(&header[..16], b"SQLite format 3\0");
        assert_eq!(header[18], 2, "database write version must be WAL");
        assert_eq!(header[19], 2, "database read version must be WAL");

        for database in [&first, &second] {
            let version: i64 = database
                .connection
                .query_row(
                    "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
                    [],
                    |row| row.get(0),
                )
                .expect("migration history is readable");
            assert_eq!(version, 7, "all schema migrations must be applied");

            let platform_id_column: i64 = database
                .connection
                .query_row(
                    "SELECT COUNT(*) FROM pragma_table_info('games') WHERE name = 'platform_id'",
                    [],
                    |row| row.get(0),
                )
                .expect("game schema is readable");
            assert_eq!(platform_id_column, 1);
        }

        drop(first);
        drop(second);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }

    #[test]
    fn games_remain_available_after_reopening_database() {
        let path = std::env::temp_dir().join(format!("amethys-{}.sqlite", uuid::Uuid::new_v4()));
        let id = {
            let database = Database::open(&path).expect("database opens");
            database
                .add_game("Persistent Game", "GOG", "/games/persistent", None)
                .expect("game is added")
                .id
        };
        let database = Database::open(&path).expect("database reopens");
        assert_eq!(
            database
                .get_game(&id)
                .expect("lookup works")
                .expect("game exists")
                .name,
            "Persistent Game"
        );
        drop(database);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn local_imports_do_not_allocate_global_catalogue_ids() {
        let path = std::env::temp_dir().join(format!(
            "amethys-local-only-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let database = Database::open(&path).expect("database opens");
        let game = database
            .add_game("Unmatched Local Game", "Unknown", "/games/local", None)
            .expect("local import succeeds");
        assert_eq!(database.get_catalogue_game_id(&game.id).unwrap(), None);
        let count: i64 = database
            .connection
            .query_row("SELECT COUNT(*) FROM catalogue_games", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            count, 0,
            "local imports must not populate the shared catalogue"
        );
        drop(database);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }

    #[test]
    fn master_catalogue_ids_are_explicit_and_external_ids_are_global() {
        let path =
            std::env::temp_dir().join(format!("amethys-master-{}.sqlite", uuid::Uuid::new_v4()));
        let database = Database::open(&path).expect("database opens");
        database
            .register_catalogue_game("42", 42, "Master Game", false)
            .unwrap();
        assert_eq!(
            database.get_catalogue_game("42").unwrap(),
            Some(CatalogueGame {
                id: "42".into(),
                name: "Master Game".into()
            })
        );
        assert!(
            database
                .set_catalogue_external_id(42, "steam", "0000570")
                .unwrap()
        );
        assert_eq!(
            database
                .get_catalogue_game_by_external_id("steam", "0000570")
                .unwrap(),
            Some(CatalogueGame {
                id: "42".into(),
                name: "Master Game".into()
            })
        );
        assert!(
            database
                .register_catalogue_game("invalid", 0, "Invalid", false)
                .is_err()
        );
        database
            .register_catalogue_game("42", 42, "Updated Master Name", false)
            .unwrap();
        assert_eq!(
            database.get_catalogue_game("42").unwrap().unwrap().name,
            "Updated Master Name"
        );
        drop(database);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }
}
