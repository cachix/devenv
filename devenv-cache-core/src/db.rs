use crate::error::{CacheError, CacheResult};
use sqlx::migrate::Migrator;
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::{error, trace};

/// Database connection manager
#[derive(Debug, Clone)]
pub struct Database {
    pool: SqlitePool,
    path: PathBuf,
}

impl Database {
    /// Create a new database connection with the given path and run migrations
    ///
    /// * `path` - Path to the SQLite database file
    /// * `migrator` - The migrator containing database migrations to apply
    pub async fn new(path: PathBuf, migrator: &Migrator) -> CacheResult<Self> {
        let options = connection_options(&path);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;

        let db = Self { pool, path };

        // Run migrations
        trace!("Running migrations");

        if let Err(err) = migrator.run(&db.pool).await {
            error!(error = %err, "Failed to migrate the database. Attempting to recreate the database.");

            // Close the existing connection
            db.pool.close().await;

            // Delete the database and associated WAL/SHM files
            remove_sqlite_files(&db.path);

            // Recreate the database and connection pool
            let options = connection_options(&db.path);
            let new_pool = SqlitePoolOptions::new()
                .max_connections(5)
                .connect_with(options)
                .await?;

            // Create a new database instance with the new pool
            let new_db = Self {
                pool: new_pool,
                path: db.path,
            };

            // Try migrations again
            if let Err(e) = migrator.run(&new_db.pool).await {
                error!("Migration failed after recreating database: {}", e);
                return Err(CacheError::Database(e.into()));
            }

            return Ok(new_db);
        }

        Ok(db)
    }

    /// Get a reference to the connection pool
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Close the database connection
    pub async fn close(self) {
        self.pool.close().await;
    }
}

/// Create SQLite connection options
fn connection_options(path: &Path) -> SqliteConnectOptions {
    SqliteConnectOptions::new()
        .filename(path)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(10))
        .create_if_missing(true)
        .foreign_keys(true)
        .pragma("wal_autocheckpoint", "1000")
        .pragma("journal_size_limit", (64 * 1024 * 1024).to_string()) // 64 MB
        .pragma("mmap_size", "134217728") // 128 MB
        .pragma("cache_size", "2000") // 2000 pages
}

/// Remove a SQLite database file and its associated WAL/SHM files.
fn remove_sqlite_files(path: &Path) {
    for suffix in ["", "-wal", "-shm"] {
        let mut file = path.as_os_str().to_owned();
        file.push(suffix);
        let _ = std::fs::remove_file(Path::new(&file));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_database_with_percent_encoded_path() {
        let temp_dir = TempDir::new().unwrap();
        let dir_with_percent = temp_dir.path().join("test%2Fdir");
        std::fs::create_dir_all(&dir_with_percent).unwrap();
        let db_path = dir_with_percent.join("test.db");

        let migrations_dir = temp_dir.path().join("migrations");
        std::fs::create_dir_all(&migrations_dir).unwrap();
        std::fs::write(
            migrations_dir.join("20250507000001_test.sql"),
            "CREATE TABLE test (id INTEGER PRIMARY KEY)",
        )
        .unwrap();

        let migrator = sqlx::migrate::Migrator::new(migrations_dir).await.unwrap();
        let db = Database::new(db_path.clone(), &migrator).await.unwrap();

        assert!(db_path.exists());
        db.close().await;
    }

    #[tokio::test]
    async fn test_database_creation() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        // Create a test schema directly for testing
        let test_schema = "CREATE TABLE test (id INTEGER PRIMARY KEY, name TEXT)";

        // Create a test migrator using the built-in migrations system
        // We'll create a temporary directory and write a migration file to it
        let migrations_dir = temp_dir.path().join("migrations");
        std::fs::create_dir_all(&migrations_dir).unwrap();

        // Write a migration file
        let migration_file = migrations_dir.join("20250507000001_test.sql");
        std::fs::write(&migration_file, test_schema).unwrap();

        // Create a migrator from the directory
        let migrator = sqlx::migrate::Migrator::new(migrations_dir).await.unwrap();

        let db = Database::new(db_path.clone(), &migrator).await.unwrap();

        // Test that the database file was created
        assert!(db_path.exists());

        // Test that we can execute queries
        sqlx::query("INSERT INTO test (name) VALUES (?)")
            .bind("test_value")
            .execute(db.pool())
            .await
            .unwrap();

        // Test query
        let row: (i64, String) = sqlx::query_as("SELECT id, name FROM test WHERE name = ?")
            .bind("test_value")
            .fetch_one(db.pool())
            .await
            .unwrap();

        assert_eq!(row.1, "test_value");

        // Close database
        db.close().await;
    }
}
