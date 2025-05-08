use crate::error::{CacheError, CacheResult};
use sqlx::migrate::{MigrateDatabase, Migrator};
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;
use tracing::{debug, error};

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
        let db_url = format!("sqlite:{}", path.display());
        let options = connection_options(&db_url)?;

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;

        let db = Self { pool, path };

        // Run migrations
        debug!("Running migrations");

        if let Err(err) = migrator.run(&db.pool).await {
            error!(error = %err, "Failed to migrate the database. Attempting to recreate the database.");

            // Close the existing connection
            db.pool.close().await;

            // Delete and recreate the database
            sqlx::Sqlite::drop_database(&db_url).await?;

            // Recreate the database and connection pool
            let options = connection_options(&db_url)?;
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
fn connection_options(db_url: &str) -> CacheResult<SqliteConnectOptions> {
    let options = SqliteConnectOptions::from_str(db_url)?
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(10))
        .create_if_missing(true)
        .foreign_keys(true)
        .pragma("wal_autocheckpoint", "1000")
        .pragma("journal_size_limit", (64 * 1024 * 1024).to_string()) // 64 MB
        .pragma("mmap_size", "134217728") // 128 MB
        .pragma("cache_size", "2000"); // 2000 pages

    Ok(options)
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
