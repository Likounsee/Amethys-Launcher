use amethys_errors::{AmethysError, ErrorCode};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;

/// A game registered in the user's local library.
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

/// SQLite database owned by the core process.
pub struct Database {
    connection: Connection,
}

impl Database {
    /// Opens or creates the database and applies all known schema migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, AmethysError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|error| AmethysError::new(
                ErrorCode::StorageOpen,
                format!("Could not create {}: {error}", parent.display()),
            ))?;
        }
        let mut connection = Connection::open(path).map_err(|error| AmethysError::new(
            ErrorCode::StorageOpen,
            format!("Could not open {}: {error}", path.display()),
        ))?;
        // A second Amethys process or another database client may briefly hold
        // a write lock. Wait for it instead of failing immediately on SQLITE_BUSY.
        connection.busy_timeout(std::time::Duration::from_secs(5)).map_err(|error|
            AmethysError::new(
                ErrorCode::StorageOpen,
                format!("Could not configure database busy timeout: {error}"),
            )
        )?;
        // Check the schema before changing persistent SQLite settings. In
        // particular, opening a database created by a newer Amethys release
        // must not silently switch its journal mode or otherwise mutate it.
        let migration_table_exists: bool = connection.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'schema_migrations'
             )",
            [],
            |row| row.get(0),
        ).map_err(|error| AmethysError::new(
            ErrorCode::StorageMigration,
            format!("Could not inspect database schema: {error}"),
        ))?;
        let detected_version: i64 = if migration_table_exists {
            connection.query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
                [],
                |row| row.get(0),
            ).map_err(|error| AmethysError::new(
                ErrorCode::StorageMigration,
                format!("Could not read migration history: {error}"),
            ))?
        } else {
            0
        };
        if detected_version > 3 {
            return Err(AmethysError::new(
                ErrorCode::StorageMigration,
                format!(
                    "Database schema version {detected_version} is newer than the supported version 3."
                ),
            ));
        }

        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = FULL;"
        ).map_err(|error| AmethysError::new(
            ErrorCode::StorageOpen,
            format!("Could not configure database: {error}"),
        ))?;

        // Keep schema creation and migration markers in one transaction. A
        // failed statement must not leave a partially initialized database.
        let transaction = connection.transaction().map_err(|error| AmethysError::new(
            ErrorCode::StorageMigration,
            format!("Could not begin database migration: {error}"),
        ))?;
        transaction.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                 version INTEGER PRIMARY KEY NOT NULL,
                 applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );"
        ).map_err(|error| AmethysError::new(
            ErrorCode::StorageMigration,
            format!("Could not create migration history: {error}"),
        ))?;

        // Re-read under the migration transaction in case another Amethys
        // process completed a migration after the read-only preflight.
        let current_version: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        ).map_err(|error| AmethysError::new(
            ErrorCode::StorageMigration,
            format!("Could not read migration history: {error}"),
        ))?;
        if current_version > 3 {
            return Err(AmethysError::new(
                ErrorCode::StorageMigration,
                format!(
                    "Database schema version {current_version} is newer than the supported version 3."
                ),
            ));
        }

        transaction.execute_batch(
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
             INSERT OR IGNORE INTO schema_migrations (version) VALUES (2);"
        ).map_err(|error| AmethysError::new(
            ErrorCode::StorageMigration,
            format!("Could not initialize database schema: {error}"),
        ))?;
        if current_version < 3 {
            transaction.execute_batch(
                "ALTER TABLE games ADD COLUMN platform_id TEXT;
                 CREATE UNIQUE INDEX IF NOT EXISTS idx_games_platform_identity
                     ON games(platform COLLATE NOCASE, platform_id)
                     WHERE platform_id IS NOT NULL;
                 INSERT OR IGNORE INTO schema_migrations (version) VALUES (3);"
            ).map_err(|error| AmethysError::new(
                ErrorCode::StorageMigration,
                format!("Could not apply game platform-ID migration: {error}"),
            ))?;
        }
        transaction.commit().map_err(|error| AmethysError::new(
            ErrorCode::StorageMigration,
            format!("Could not commit database migration: {error}"),
        ))?;
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
            return Err(storage_error("Game name, platform and install path must not be empty."));
        }
        if platform_id.is_some_and(str::is_empty) {
            return Err(storage_error("Platform game ID must not be empty when provided."));
        }
        // SQLite accepts embedded NUL bytes in TEXT values, but downstream
        // filesystem and process APIs cannot use such paths safely. Reject
        // them at the persistence boundary rather than storing unusable data.
        if [name, platform, install_path]
            .iter()
            .any(|value| value.contains('\\0'))
            || platform_id.is_some_and(|value| value.contains('\\0'))
            || executable.is_some_and(|value| value.contains('\\0'))
        {
            return Err(storage_error("Game metadata must not contain NUL characters."));
        }

        let id = uuid::Uuid::new_v4().to_string();
        self.connection.execute(
            "INSERT INTO games (id, name, platform, platform_id, install_path, executable)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, name, platform, platform_id, install_path, executable],
        ).map_err(|error| storage_error(format!("Could not add game: {error}")))?;
        self.get_game(&id)?.ok_or_else(|| storage_error("Added game could not be read back."))
    }

    /// Finds a game using the identifier assigned by its source platform.
    pub fn get_game_by_platform_id(
        &self,
        platform: &str,
        platform_id: &str,
    ) -> Result<Option<GameEntry>, AmethysError> {
        self.connection.query_row(
            "SELECT id, name, platform, platform_id, install_path, executable, added_at
             FROM games WHERE platform = ?1 COLLATE NOCASE AND platform_id = ?2",
            params![platform.trim(), platform_id.trim()],
            game_from_row,
        ).optional().map_err(|error| storage_error(format!("Could not find game by platform ID: {error}")))
    }

    /// Finds one game by its stable identifier.
    pub fn get_game(&self, id: &str) -> Result<Option<GameEntry>, AmethysError> {
        self.connection.query_row(
            "SELECT id, name, platform, platform_id, install_path, executable, added_at
             FROM games WHERE id = ?1",
            [id],
            game_from_row,
        ).optional().map_err(|error| storage_error(format!("Could not read game: {error}")))
    }

    /// Returns the library ordered by name, without depending on a platform client.
    pub fn list_games(&self) -> Result<Vec<GameEntry>, AmethysError> {
        let mut statement = self.connection.prepare(
            "SELECT id, name, platform, platform_id, install_path, executable, added_at
             FROM games ORDER BY name COLLATE NOCASE, id"
        ).map_err(|error| storage_error(format!("Could not prepare game list: {error}")))?;
        let rows = statement.query_map([], game_from_row)
            .map_err(|error| storage_error(format!("Could not list games: {error}")))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| storage_error(format!("Could not read game list: {error}")))
    }

    /// Removes a game. Returns false when its identifier was not present.
    pub fn remove_game(&self, id: &str) -> Result<bool, AmethysError> {
        let changed = self.connection.execute("DELETE FROM games WHERE id = ?1", [id])
            .map_err(|error| storage_error(format!("Could not remove game: {error}")))?;
        Ok(changed != 0)
    }

    #[must_use]
    pub const fn connection(&self) -> &Connection { &self.connection }
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

fn storage_error(message: impl Into<String>) -> AmethysError {
    AmethysError::new(ErrorCode::StorageQuery, message)
}

#[cfg(test)]
mod tests {
    use super::Database;

    fn temporary_database() -> (Database, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("amethys-{}.sqlite", uuid::Uuid::new_v4()));
        let database = Database::open(&path).expect("database opens");
        (database, path)
    }

    #[test]
    fn database_opens_from_a_relative_filename() {
        let filename = format!("amethys-{}.sqlite", uuid::Uuid::new_v4());
        let path = std::path::Path::new(&filename);
        let database = Database::open(path).expect("relative database path opens");
        let count: i64 = database.connection()
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| row.get(0))
            .expect("migration table readable");
        assert_eq!(count, 3);
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
            connection.execute_batch("CREATE TABLE schema_migrations (wrong TEXT);")
                .expect("incompatible migration table is created");
        }

        let error = match Database::open(&path) {
            Err(error) => error,
            Ok(_) => panic!("incompatible schema must fail"),
        };
        assert_eq!(error.code(), amethys_errors::ErrorCode::StorageMigration);

        let connection = rusqlite::Connection::open(&path).expect("fixture database reopens");
        let app_settings_exists: i64 = connection.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'app_settings'",
            [],
            |row| row.get(0),
        ).expect("schema catalog is readable");
        assert_eq!(app_settings_exists, 0, "partial table creation must be rolled back");
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
            connection.execute_batch(
                "CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY NOT NULL,
                    applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );
                INSERT INTO schema_migrations (version) VALUES (4);"
            ).expect("future schema is created");
        }

        let error = match Database::open(&path) {
            Err(error) => error,
            Ok(_) => panic!("newer schema must be rejected"),
        };
        assert_eq!(error.code(), amethys_errors::ErrorCode::StorageMigration);
        assert!(error.message().contains("newer than the supported version 3"));

        let connection = rusqlite::Connection::open(&path).expect("fixture database reopens");
        let games_exists: i64 = connection.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'games'",
            [],
            |row| row.get(0),
        ).expect("schema catalog is readable");
        assert_eq!(games_exists, 0, "future database must remain untouched");
        let journal_mode: String = connection.query_row(
            "PRAGMA journal_mode", [], |row| row.get(0)
        ).expect("journal mode is readable");
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
            connection.execute_batch(
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
                VALUES ('legacy-id', 'Legacy Game', 'Steam', '/games/legacy');"
            ).expect("version two fixture is created");
        }

        let database = Database::open(&path).expect("version two database migrates");
        let game = database.get_game("legacy-id").expect("legacy lookup works").expect("legacy game remains");
        assert_eq!(game.name, "Legacy Game");
        assert_eq!(game.platform_id, None);
        let version: i64 = database.connection().query_row(
            "SELECT MAX(version) FROM schema_migrations", [], |row| row.get(0)
        ).expect("version is readable");
        assert_eq!(version, 3);
        drop(database);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }

    #[test]
    fn game_metadata_rejects_embedded_nul_characters() {
        let (database, path) = temporary_database();
        assert!(database.add_game(
            "Bad\\0Name", "Steam", "/games/example", None
        ).is_err());
        assert!(database.add_game(
            "Example", "Steam", "/games/\\0example", None
        ).is_err());
        assert!(database.add_game(
            "Example", "Steam", "/games/example", Some("game\\0.exe")
        ).is_err());
        assert!(database.add_game_with_platform_id(
            "Example", "Steam", Some("57\\0"), "/games/example", None
        ).is_err());
        assert!(database.list_games().expect("list remains readable").is_empty());
        drop(database);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }

    #[test]
    fn optional_executable_is_trimmed_and_empty_values_become_none() {
        let (database, path) = temporary_database();
        let game = database.add_game(
            " Example ", " Steam ", "/games/example", Some("  bin/game.exe  ")
        ).expect("valid game is added");
        assert_eq!(game.name, "Example");
        assert_eq!(game.platform, "Steam");
        assert_eq!(game.executable.as_deref(), Some("bin/game.exe"));

        let without_executable = database.add_game(
            "Another", "Steam", "/games/another", Some("   ")
        ).expect("blank optional executable is accepted");
        assert_eq!(without_executable.executable, None);
        drop(database);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }

    #[test]
    fn platform_ids_are_saved_and_unique_per_platform() {
        let (database, path) = temporary_database();
        let game = database.add_game_with_platform_id(
            "Example Game", "Steam", Some("570"), "/games/example", None
        ).expect("game with platform ID is added");
        assert_eq!(game.platform_id.as_deref(), Some("570"));
        assert_eq!(
            database.get_game_by_platform_id("steam", "570").expect("lookup works"),
            Some(game)
        );
        assert!(database.add_game_with_platform_id(
            "Duplicate", "Steam", Some("570"), "/games/duplicate", None
        ).is_err());
        assert!(database.add_game_with_platform_id(
            "Duplicate with different casing", "steam", Some("570"), "/games/duplicate-case", None
        ).is_err());
        // The same identifier may exist on a different platform.
        assert!(database.add_game_with_platform_id(
            "Other Platform Game", "Epic Games", Some("570"), "/games/other", None
        ).is_ok());
        drop(database);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }

    #[test]
    fn database_uses_full_synchronous_commits_for_durability() {
        let (database, path) = temporary_database();
        let synchronous: i64 = database.connection()
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
        let timeout_ms: i64 = database.connection()
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
        let count: i64 = database.connection()
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| row.get(0))
            .expect("migration table readable");
        assert_eq!(count, 3);
        drop(database);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn games_can_be_added_listed_found_and_removed() {
        let (database, path) = temporary_database();
        let game = database.add_game("  Example Game  ", "Steam", "/games/example", Some("game.exe"))
            .expect("game is added");
        assert_eq!(game.name, "Example Game");
        assert_eq!(database.get_game(&game.id).expect("lookup works"), Some(game.clone()));
        assert_eq!(database.list_games().expect("list works"), vec![game.clone()]);
        assert!(database.remove_game(&game.id).expect("remove works"));
        assert!(!database.remove_game(&game.id).expect("second remove works"));
        assert!(database.get_game(&game.id).expect("lookup works").is_none());
        drop(database);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn games_reject_missing_required_fields() {
        let (database, path) = temporary_database();
        assert!(database.add_game(" ", "Steam", "/games/example", None).is_err());
        assert!(database.add_game("Example", "", "/games/example", None).is_err());
        assert!(database.add_game("Example", "Steam", " ", None).is_err());
        drop(database);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn games_remain_available_after_reopening_database() {
        let path = std::env::temp_dir().join(format!("amethys-{}.sqlite", uuid::Uuid::new_v4()));
        let id = {
            let database = Database::open(&path).expect("database opens");
            database.add_game("Persistent Game", "GOG", "/games/persistent", None)
                .expect("game is added").id
        };
        let database = Database::open(&path).expect("database reopens");
        assert_eq!(database.get_game(&id).expect("lookup works").expect("game exists").name, "Persistent Game");
        drop(database);
        let _ = std::fs::remove_file(path);
    }
}
