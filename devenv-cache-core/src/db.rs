use crate::error::{CacheError, CacheResult};
use std::path::{Path, PathBuf};
use tracing::{error, trace};
use turso::Builder;

/// Database connection manager
#[derive(Debug, Clone)]
pub struct Database {
    db: turso::Database,
    path: PathBuf,
}

/// Migration definition
pub struct Migration {
    pub version: &'static str,
    pub sql: &'static str,
}

impl Database {
    /// Create a new database connection with the given path and run migrations
    ///
    /// * `path` - Path to the SQLite database file
    /// * `migrations` - The migrations to apply (in order)
    pub async fn new(path: PathBuf, migrations: &[Migration]) -> CacheResult<Self> {
        let db = Builder::new_local(path.to_str().ok_or_else(|| {
            CacheError::InvalidPath(path.clone())
        })?)
        .build()
        .await
        .map_err(|e| CacheError::Database(e.to_string()))?;

        let database = Self {
            db: db.clone(),
            path,
        };

        // Run migrations
        trace!("Running migrations");

        if let Err(err) = database.run_migrations(migrations).await {
            error!(error = %err, "Failed to migrate the database. Attempting to recreate the database.");

            // Delete and recreate the database
            if database.path.exists() {
                std::fs::remove_file(&database.path)?;
            }

            // Recreate the database
            let new_db = Builder::new_local(database.path.to_str().ok_or_else(|| {
                CacheError::InvalidPath(database.path.clone())
            })?)
            .build()
            .await
            .map_err(|e| CacheError::Database(e.to_string()))?;

            let new_database = Self {
                db: new_db,
                path: database.path,
            };

            // Try migrations again
            if let Err(e) = new_database.run_migrations(migrations).await {
                error!("Migration failed after recreating database: {}", e);
                return Err(e);
            }

            return Ok(new_database);
        }

        Ok(database)
    }

    /// Run database migrations
    async fn run_migrations(&self, migrations: &[Migration]) -> CacheResult<()> {
        // Create migrations table if it doesn't exist
        let conn = self.db.connect().map_err(|e| CacheError::Database(e.to_string()))?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS _migrations (version TEXT PRIMARY KEY, applied_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')))",
            ()
        )
        .await
        .map_err(|e| CacheError::Database(e.to_string()))?;

        // Apply each migration
        for migration in migrations {
            // Check if migration was already applied
            let mut rows = conn
                .query("SELECT version FROM _migrations WHERE version = ?1", (migration.version,))
                .await
                .map_err(|e| CacheError::Database(e.to_string()))?;

            let already_applied = rows.next().await.map_err(|e| CacheError::Database(e.to_string()))?.is_some();

            if !already_applied {
                trace!("Applying migration: {}", migration.version);

                // Execute migration SQL
                conn.execute(migration.sql, ())
                    .await
                    .map_err(|e| CacheError::Database(format!("Migration {} failed: {}", migration.version, e)))?;

                // Record migration
                conn.execute(
                    "INSERT INTO _migrations (version) VALUES (?1)",
                    (migration.version,)
                )
                .await
                .map_err(|e| CacheError::Database(e.to_string()))?;
            }
        }

        Ok(())
    }

    /// Get a reference to the database
    pub fn db(&self) -> &turso::Database {
        &self.db
    }

    /// Get a connection from the database
    pub fn connect(&self) -> CacheResult<turso::Connection> {
        self.db.connect().map_err(|e| CacheError::Database(e.to_string()))
    }

    /// Close the database connection
    pub async fn close(self) {
        // Turso databases close automatically when dropped
        drop(self.db);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Test database creation with migrations
    #[tokio::test]
    async fn test_database_creation() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        // Create a test migration
        let migrations = vec![
            Migration {
                version: "20250507000001_test",
                sql: "CREATE TABLE test (id INTEGER PRIMARY KEY, name TEXT)",
            },
        ];

        let db = Database::new(db_path.clone(), &migrations).await.unwrap();

        // Test that the database file was created
        assert!(db_path.exists());

        // Test that we can execute queries
        let conn = db.connect().unwrap();
        conn.execute("INSERT INTO test (name) VALUES (?1)", ("test_value",))
            .await
            .unwrap();

        // Test query
        let mut rows = conn.query("SELECT id, name FROM test WHERE name = ?1", ("test_value",))
            .await
            .unwrap();

        let row = rows.next().await.unwrap().unwrap();
        let id: i64 = row.get(0).unwrap();
        let name: String = row.get(1).unwrap();

        assert_eq!(name, "test_value");
        assert!(id > 0);

        // Close database
        db.close().await;
    }
}
