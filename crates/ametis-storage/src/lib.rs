use ametis_errors::{AmetisError, ErrorCode};
use rusqlite::Connection;
use std::path::Path;

/// SQLite database owned by the core process.
pub struct Database {
    connection: Connection,
}

impl Database {
    /// Opens or creates the database and initializes the first schema version.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, AmetisError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| AmetisError::new(
                ErrorCode::StorageOpen,
                format!("Could not create {}: {error}", parent.display()),
            ))?;
        }
        let connection = Connection::open(path).map_err(|error| AmetisError::new(
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
             INSERT OR IGNORE INTO schema_migrations (version) VALUES (1);"
        ).map_err(|error| AmetisError::new(
            ErrorCode::StorageMigration,
            format!("Could not initialize database schema: {error}"),
        ))?;
        Ok(Self { connection })
    }

    #[must_use]
    pub const fn connection(&self) -> &Connection { &self.connection }
}

#[cfg(test)]
mod tests {
    use super::Database;

    #[test]
    fn database_initializes_schema() {
        let path = std::env::temp_dir().join(format!("ametis-{}.sqlite", uuid::Uuid::new_v4()));
        let database = Database::open(&path).expect("database opens");
        let count: i64 = database.connection()
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| row.get(0))
            .expect("migration table readable");
        assert_eq!(count, 1);
        drop(database);
        let _ = std::fs::remove_file(path);
    }
}
