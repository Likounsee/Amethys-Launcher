use amethys_errors::{AmethysError, ErrorCode};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;

/// A game registered in the user's local library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameEntry {
    pub id: String,
    pub name: String,
    pub platform: String,
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
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| AmethysError::new(
                ErrorCode::StorageOpen,
                format!("Could not create {}: {error}", parent.display()),
            ))?;
        }
        let connection = Connection::open(path).map_err(|error| AmethysError::new(
            ErrorCode::StorageOpen,
            format!("Could not open {}: {error}", path.display()),
        ))?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS schema_migrations (
                 version INTEGER PRIMARY KEY NOT NULL,
                 applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );
             CREATE TABLE IF NOT EXISTS app_settings (
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
        Ok(Self { connection })
    }

    /// Adds a game to the local library and returns its generated identifier.
    pub fn add_game(
        &self,
        name: &str,
        platform: &str,
        install_path: &str,
        executable: Option<&str>,
    ) -> Result<GameEntry, AmethysError> {
        let name = name.trim();
        let platform = platform.trim();
        let install_path = install_path.trim();
        if name.is_empty() || platform.is_empty() || install_path.is_empty() {
            return Err(storage_error("Game name, platform and install path must not be empty."));
        }

        let id = uuid::Uuid::new_v4().to_string();
        self.connection.execute(
            "INSERT INTO games (id, name, platform, install_path, executable)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, name, platform, install_path, executable],
        ).map_err(|error| storage_error(format!("Could not add game: {error}")))?;
        self.get_game(&id)?.ok_or_else(|| storage_error("Added game could not be read back."))
    }

    /// Finds one game by its stable identifier.
    pub fn get_game(&self, id: &str) -> Result<Option<GameEntry>, AmethysError> {
        self.connection.query_row(
            "SELECT id, name, platform, install_path, executable, added_at
             FROM games WHERE id = ?1",
            [id],
            game_from_row,
        ).optional().map_err(|error| storage_error(format!("Could not read game: {error}")))
    }

    /// Returns the library ordered by name, without depending on a platform client.
    pub fn list_games(&self) -> Result<Vec<GameEntry>, AmethysError> {
        let mut statement = self.connection.prepare(
            "SELECT id, name, platform, install_path, executable, added_at
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
        install_path: row.get(3)?,
        executable: row.get(4)?,
        added_at: row.get(5)?,
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
    fn database_initializes_schema() {
        let (database, path) = temporary_database();
        let count: i64 = database.connection()
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| row.get(0))
            .expect("migration table readable");
        assert_eq!(count, 2);
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
