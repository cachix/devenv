use crate::error::{CacheError, CacheResult};
use std::path::{Path, PathBuf};
use tracing::{error, trace};
use turso::{Builder, Connection, Database as TursoDatabase, params};

/// Database connection manager
#[derive(Debug, Clone)]
pub struct Database {
    db: TursoDatabase,
    path: PathBuf,
}

impl Database {
    /// Create a new database connection with the given path and run migrations
    ///
    /// * `path` - Path to the SQLite database file
    /// * `migrations_dir` - Path to the directory containing migration files
    pub async fn new(path: PathBuf, migrations_dir: &Path) -> CacheResult<Self> {
        let db_path_str = path
            .to_str()
            .ok_or_else(|| CacheError::Io(std::io::Error::other("Invalid database path")))?;

        let migrations_dir_str = migrations_dir.to_str().ok_or_else(|| {
            CacheError::Io(std::io::Error::other("Invalid migrations directory path"))
        })?;

        trace!("Opening database with Turso");

        // Run migrations using geni before opening the database
        trace!("Running migrations from {:?}", migrations_dir);

        let db_url = format!("sqlite://{}", db_path_str);

        if let Err(err) = geni::migrate_database(
            db_url.clone(),
            None,                          // No token for local file
            "geni_migrations".to_string(), // Migration table name
            migrations_dir_str.to_string(),
            "schema.sql".to_string(), // Schema dump file (not used)
            Some(30),                 // Timeout
            false,                    // Don't dump schema
        )
        .await
        {
            error!(error = %err, "Failed to migrate the database. Attempting to recreate the database.");

            // Delete and recreate the database
            if let Err(e) = std::fs::remove_file(db_path_str) {
                error!("Failed to remove old database: {}", e);
            }

            // Try migrations again
            if let Err(e) = geni::migrate_database(
                db_url,
                None,
                "geni_migrations".to_string(),
                migrations_dir_str.to_string(),
                "schema.sql".to_string(),
                Some(30),
                false,
            )
            .await
            {
                error!("Migration failed after recreating database: {}", e);
                return Err(CacheError::Io(std::io::Error::other(format!(
                    "Migration failed: {}",
                    e
                ))));
            }
        }

        // Open the database with Turso after migrations are complete
        let turso_db = Builder::new_local(db_path_str).build().await?;

        let db = Self { db: turso_db, path };

        Ok(db)
    }

    /// Get a connection to the database
    pub async fn connect(&self) -> CacheResult<Connection> {
        let conn = self.db.connect()?;

        // Try to enable foreign keys for this connection
        // Foreign keys are disabled by default in SQLite/libSQL
        // Try different syntax variations as turso may be picky about the format
        if conn.execute("PRAGMA foreign_keys = ON", ()).await.is_err() {
            if conn.execute("PRAGMA foreign_keys=ON", ()).await.is_err() {
                let _ = conn.execute("PRAGMA foreign_keys = 1", ()).await;
            }
        }

        // If all attempts fail, foreign key constraints may not be enforced
        // See: https://github.com/libsql/sqld/issues/764

        Ok(conn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use turso::params;

    /// Test database creation with migrations
    #[tokio::test]
    async fn test_database_creation() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        // Create test migration files
        // Note: geni expects .up.sql extension
        let migrations_dir = temp_dir.path().join("migrations");
        std::fs::create_dir_all(&migrations_dir).unwrap();
        let migration_file = migrations_dir.join("20250507000001_test.up.sql");
        std::fs::write(
            &migration_file,
            "CREATE TABLE test (id INTEGER PRIMARY KEY, name TEXT)",
        )
        .unwrap();

        // Create database with migrations
        let db = Database::new(db_path.clone(), &migrations_dir)
            .await
            .unwrap();

        // Test that the database file was created
        assert!(db_path.exists());

        // Test that we can execute queries
        let conn = db.connect().await.unwrap();
        conn.execute("INSERT INTO test (name) VALUES (?)", params!["test_value"])
            .await
            .unwrap();

        // Test query
        let mut stmt = conn
            .prepare("SELECT id, name FROM test WHERE name = ?")
            .await
            .unwrap();
        let mut rows = stmt.query(params!["test_value"]).await.unwrap();
        let row = rows.next().await.unwrap().unwrap();

        let name: String = row.get(1).unwrap();
        assert_eq!(name, "test_value");
    }

    /// Minimal test case demonstrating CASCADE DELETE bug in turso 0.2.2
    ///
    /// This test shows that PRAGMA foreign_keys doesn't work properly,
    /// which means CASCADE DELETE constraints are not enforced.
    ///
    /// Expected behavior: Deleting parent should cascade delete child
    /// Actual behavior: Child record remains after parent is deleted
    #[tokio::test]
    async fn test_cascade_delete_bug() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test_cascade.db");

        // Create migration with foreign key CASCADE DELETE constraint
        let migrations_dir = temp_dir.path().join("migrations");
        std::fs::create_dir_all(&migrations_dir).unwrap();
        let migration_file = migrations_dir.join("20250507000002_cascade_test.up.sql");
        std::fs::write(
            &migration_file,
            r#"
CREATE TABLE parent (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);

CREATE TABLE child (
    id INTEGER PRIMARY KEY,
    parent_id INTEGER NOT NULL,
    data TEXT,
    FOREIGN KEY (parent_id) REFERENCES parent(id) ON DELETE CASCADE
);
"#,
        )
        .unwrap();

        // Create database
        let db = Database::new(db_path.clone(), &migrations_dir)
            .await
            .unwrap();

        let conn = db.connect().await.unwrap();

        // Insert parent record
        conn.execute(
            "INSERT INTO parent (id, name) VALUES (?, ?)",
            params![1, "test_parent"],
        )
        .await
        .unwrap();

        // Insert child record
        conn.execute(
            "INSERT INTO child (id, parent_id, data) VALUES (?, ?, ?)",
            params![1, 1, "test_child"],
        )
        .await
        .unwrap();

        // Verify child exists
        let mut stmt = conn
            .prepare("SELECT COUNT(*) FROM child WHERE parent_id = ?")
            .await
            .unwrap();
        let mut rows = stmt.query(params![1]).await.unwrap();
        let row = rows.next().await.unwrap().unwrap();
        let count: i64 = row.get(0).unwrap();
        assert_eq!(count, 1, "Child should exist before parent deletion");

        // Delete parent - this should CASCADE DELETE the child, but it won't in turso 0.2.2
        conn.execute("DELETE FROM parent WHERE id = ?", params![1])
            .await
            .unwrap();

        // Check if child still exists (BUG: it will still exist)
        let mut stmt = conn
            .prepare("SELECT COUNT(*) FROM child WHERE parent_id = ?")
            .await
            .unwrap();
        let mut rows = stmt.query(params![1]).await.unwrap();
        let row = rows.next().await.unwrap().unwrap();
        let count_after: i64 = row.get(0).unwrap();

        // This assertion will FAIL, demonstrating the bug
        // In a properly functioning database with foreign keys enabled, count_after should be 0
        assert_eq!(
            count_after, 0,
            "CASCADE DELETE bug: child record still exists after parent deletion (count={})",
            count_after
        );
    }
}
