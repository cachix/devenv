use crate::error::{CacheError, CacheResult};
pub use include_dir::{Dir, include_dir};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::TempDir;
use tracing::{error, trace};
use turso::{Builder, Connection, Database as TursoDb};

/// Database connection manager
#[derive(Debug, Clone)]
pub struct Database {
    db: Arc<TursoDb>,
    path: PathBuf,
}

impl Database {
    /// Create a new database connection with the given path and run migrations
    ///
    /// * `path` - Path to the SQLite database file
    /// * `migrations_folder` - Path to the folder containing SQL migration files
    pub async fn new(path: PathBuf, migrations_folder: &Path) -> CacheResult<Self> {
        let db_path_str = path.to_string_lossy().to_string();
        let migrations_folder_str = migrations_folder
            .to_str()
            .ok_or_else(|| CacheError::initialization("Invalid migrations folder path"))?;

        // Run migrations first with geni (before we open our connection)
        // This avoids locking issues since geni opens its own connection
        trace!("Running migrations");
        if let Err(err) = run_migrations(&db_path_str, migrations_folder_str).await {
            error!(error = %err, "Failed to migrate the database. Attempting to recreate the database.");

            // Delete the database file and try again
            if path.exists() {
                std::fs::remove_file(&path)?;
            }

            // Try migrations again on fresh database
            if let Err(e) = run_migrations(&db_path_str, migrations_folder_str).await {
                error!("Migration failed after recreating database: {}", e);
                return Err(CacheError::initialization(format!(
                    "Migration failed: {}",
                    e
                )));
            }
        }

        // Now create our database connection after migrations are done
        let db = Builder::new_local(&db_path_str)
            .build()
            .await
            .map_err(|e| CacheError::Database(e.into()))?;

        let database = Self {
            db: Arc::new(db),
            path,
        };

        // Verify connection works with pragmas
        let _ = database.connect().await?;

        Ok(database)
    }

    /// Create a new database connection with embedded migrations.
    ///
    /// This writes the migrations to a temporary directory and runs them.
    /// Use this when migrations are embedded in the binary at compile time
    /// using `include_dir!`.
    ///
    /// * `path` - Path to the SQLite database file
    /// * `migrations_dir` - Directory embedded at compile time containing migration files
    pub async fn new_with_embedded_migrations(
        path: PathBuf,
        migrations_dir: &Dir<'static>,
    ) -> CacheResult<Self> {
        // Write embedded migrations to a temp directory
        let temp_dir = write_embedded_migrations(migrations_dir)?;
        let migrations_folder = temp_dir.path();

        let db_path_str = path.to_string_lossy().to_string();
        let migrations_folder_str = migrations_folder
            .to_str()
            .ok_or_else(|| CacheError::initialization("Invalid migrations folder path"))?;

        // Run migrations first with geni (before we open our connection)
        trace!("Running embedded migrations");
        if let Err(err) = run_migrations(&db_path_str, migrations_folder_str).await {
            error!(error = %err, "Failed to migrate the database. Attempting to recreate the database.");

            // Delete the database file and try again
            if path.exists() {
                std::fs::remove_file(&path)?;
            }

            // Try migrations again on fresh database
            if let Err(e) = run_migrations(&db_path_str, migrations_folder_str).await {
                error!("Migration failed after recreating database: {}", e);
                return Err(CacheError::initialization(format!(
                    "Migration failed: {}",
                    e
                )));
            }
        }

        // Now create our database connection after migrations are done
        let db = Builder::new_local(&db_path_str)
            .build()
            .await
            .map_err(|e| CacheError::Database(e.into()))?;

        let database = Self {
            db: Arc::new(db),
            path,
        };

        // Verify connection works with pragmas
        let _ = database.connect().await?;

        // temp_dir is dropped here, cleaning up the temporary migrations folder
        Ok(database)
    }

    /// Get a new connection from the database with pragmas configured
    pub async fn connect(&self) -> CacheResult<Connection> {
        let conn = self
            .db
            .connect()
            .map_err(|e| CacheError::Database(e.into()))?;
        configure_connection(&conn).await?;
        Ok(conn)
    }

    /// Get a reference to the underlying turso Database
    pub fn inner(&self) -> &TursoDb {
        &self.db
    }

    /// Get the path to the database file
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Write embedded migrations to a temporary directory.
fn write_embedded_migrations(migrations_dir: &Dir<'static>) -> CacheResult<TempDir> {
    let temp_dir = TempDir::new()
        .map_err(|e| CacheError::initialization(format!("Failed to create temp dir: {}", e)))?;

    for file in migrations_dir.files() {
        let file_path = temp_dir.path().join(file.path());
        std::fs::write(&file_path, file.contents()).map_err(|e| {
            CacheError::initialization(format!(
                "Failed to write migration {}: {}",
                file.path().display(),
                e
            ))
        })?;
    }

    Ok(temp_dir)
}

/// Run migrations using geni
async fn run_migrations(db_path: &str, migrations_folder: &str) -> CacheResult<()> {
    // Use turso:// scheme to route to geni's TursoDriver
    let db_url = format!("turso://{}", db_path);
    geni::migrate_database(
        db_url,
        None,                       // token (not needed for local SQLite)
        "schema_migrations".into(), // migrations table name
        migrations_folder.into(),   // migrations folder
        String::new(),              // schema file (empty = don't generate)
        None,                       // wait timeout
        false,                      // don't dump schema
    )
    .await
    .map_err(|e| CacheError::initialization(format!("Migration failed: {}", e)))
}

/// Configure SQLite connection with performance pragmas
async fn configure_connection(conn: &Connection) -> CacheResult<()> {
    // Turso/libsql handles most SQLite optimizations internally.
    // Only set foreign_keys which is a standard SQLite setting.
    // Note: turso's execute() doesn't support PRAGMA statements that return rows,
    // so we use a simple execute and ignore the result for pragmas that might fail.
    let _ = conn.execute("PRAGMA foreign_keys = ON", ()).await;
    let _ = conn.execute("PRAGMA busy_timeout = 10000", ()).await;
    Ok(())
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
        let test_schema = "CREATE TABLE test (id INTEGER PRIMARY KEY, name TEXT);";

        // Create a temporary directory and write a migration file to it
        let migrations_dir = temp_dir.path().join("migrations");
        std::fs::create_dir_all(&migrations_dir).unwrap();

        // Write a migration file (geni format: YYYYMMDDHHMMSS_name.up.sql)
        let migration_file = migrations_dir.join("20250507000001_test.up.sql");
        std::fs::write(&migration_file, test_schema).unwrap();

        let db = Database::new(db_path.clone(), &migrations_dir)
            .await
            .unwrap();

        // Test that the database file was created
        assert!(db_path.exists());

        // Get a connection and execute queries
        let conn = db.connect().await.unwrap();

        // Test that we can execute queries
        conn.execute(
            "INSERT INTO test (name) VALUES (?)",
            turso::params!["test_value"],
        )
        .await
        .unwrap();

        // Test query
        let mut stmt = conn
            .prepare("SELECT id, name FROM test WHERE name = ?")
            .await
            .unwrap();
        let mut rows = stmt.query(turso::params!["test_value"]).await.unwrap();
        let row = rows.next().await.unwrap().unwrap();

        let name: String = row.get(1).unwrap();
        assert_eq!(name, "test_value");
    }
}
