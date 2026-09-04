use crate::error::{CacheError, CacheResult};
use libsqlite3_sys::{SQLITE_BUSY, SQLITE_IOERR_DELETE_NOENT, SQLITE_IOERR_SHMMAP, SQLITE_LOCKED};
use sqlx::migrate::{MigrateError, Migrator};
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::{debug, error, trace, warn};

/// Database connection manager
#[derive(Debug, Clone)]
pub struct Database {
    pool: SqlitePool,
    _path: PathBuf,
}

impl Database {
    /// Create a new database connection with the given path and run migrations
    ///
    /// * `path` - Path to the SQLite database file
    /// * `migrator` - The migrator containing database migrations to apply
    pub async fn new(path: PathBuf, migrator: &Migrator) -> CacheResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Serialize create + migrate across processes. Concurrent cold
        // `devenv shell` entries race on this window: sqlite3_open(CREATE) and
        // PRAGMA journal_mode=WAL can return SQLITE_BUSY before busy_timeout is
        // installed, and a migration error used to delete the database out from
        // under the process that created it (#3133).
        let _init_lock = acquire_init_lock(&path).await?;

        trace!("Running migrations");

        // Try WAL journal mode first, falling back to DELETE (the default) on
        // SQLITE_IOERR_SHMMAP. WAL is preferred for concurrency, but requires
        // shared-memory support the VFS doesn't always have (e.g. some
        // virtiofs/9p/network mounts). See
        // https://github.com/cachix/devenv/issues/2947.
        let pool = match open_pool(&path, SqliteJournalMode::Wal).await {
            Ok(pool) => pool,
            Err(e) if is_shmmap_error(&e) => fall_back_to_delete_mode(&path, &e).await?,
            Err(e) => return Err(CacheError::Database(e)),
        };

        if let Err(err) = migrator.run(&pool).await {
            // Same shm-mmap failure, just surfacing during migration instead of
            // at connect time (SQLite can defer opening the `-shm` file until
            // the first real transaction).
            if migrate_error_is_shmmap(&err) {
                pool.close().await;
                let pool = fall_back_to_delete_mode(&path, &err).await?;
                migrator
                    .run(&pool)
                    .await
                    .map_err(|e| CacheError::Database(e.into()))?;
                return Ok(Self { pool, _path: path });
            }

            // A lock/busy error means another connection is using the file.
            // Never delete it — that is how concurrent cold entry turned a
            // recoverable SQLITE_BUSY into SQLITE_IOERR_DELETE_NOENT (#3133).
            if migrate_error_is_busy(&err) {
                warn!(
                    error = %err,
                    path = %path.display(),
                    "database locked during migration, retrying without recreating"
                );
                migrator
                    .run(&pool)
                    .await
                    .map_err(|e| CacheError::Database(e.into()))?;
                return Ok(Self { pool, _path: path });
            }

            // Some other migration failure (corruption, a partially-applied
            // prior migration run, etc). Delete and recreate once. No other
            // process is in Database::new while we hold the init lock; this
            // remains last-resort recovery if a process that already opened
            // the pool is still using the file.
            error!(error = %err, "Failed to migrate the database. Attempting to recreate the database.");
            pool.close().await;
            remove_sqlite_files(&path);

            let new_pool = open_pool(&path, SqliteJournalMode::Wal)
                .await
                .map_err(CacheError::Database)?;
            if let Err(e) = migrator.run(&new_pool).await {
                error!("Migration failed after recreating database: {}", e);
                return Err(CacheError::Database(e.into()));
            }
            return Ok(Self {
                pool: new_pool,
                _path: path,
            });
        }

        Ok(Self { pool, _path: path })
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

/// Connections kept open per `Database`. Also referenced by tests that need to
/// force real contention on the pool.
const MAX_CONNECTIONS: u32 = 5;

async fn open_pool(
    path: &Path,
    journal_mode: SqliteJournalMode,
) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .journal_mode(journal_mode)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(10))
        .create_if_missing(true)
        .foreign_keys(true)
        .pragma("wal_autocheckpoint", "1000")
        .pragma("journal_size_limit", (64 * 1024 * 1024).to_string()) // 64 MB
        .pragma("cache_size", "2000"); // 2000 pages

    SqlitePoolOptions::new()
        .max_connections(MAX_CONNECTIONS)
        .connect_with(options)
        .await
}

/// Log the fallback and reopen in DELETE mode. Shared by the connect-time and
/// migrate-time failure sites, which hit the same class of error.
async fn fall_back_to_delete_mode(
    path: &Path,
    error: impl std::fmt::Display,
) -> CacheResult<SqlitePool> {
    warn!(
        %error,
        path = %path.display(),
        "got SQLITE_IOERR_SHMMAP, falling back to DELETE journal mode (reduced concurrency)"
    );
    remove_sqlite_files(path);
    open_pool(path, SqliteJournalMode::Delete)
        .await
        .map_err(CacheError::Database)
}

/// Remove a SQLite database file and its associated WAL/SHM files.
fn remove_sqlite_files(path: &Path) {
    for suffix in ["", "-wal", "-shm"] {
        let mut file = path.as_os_str().to_owned();
        file.push(suffix);
        let _ = std::fs::remove_file(Path::new(&file));
    }
}

/// True if this is exactly `SQLITE_IOERR_SHMMAP` -- WAL mode failing to
/// `mmap(MAP_SHARED)` its `-shm` coordination file. See
/// https://github.com/cachix/devenv/issues/2947.
fn is_shmmap_error(error: &sqlx::Error) -> bool {
    sqlite_error_code(error) == Some(SQLITE_IOERR_SHMMAP)
}

fn sqlite_error_code(error: &sqlx::Error) -> Option<i32> {
    match error {
        sqlx::Error::Database(db_err) => db_err.code()?.parse().ok(),
        _ => None,
    }
}

/// SQLITE_BUSY / SQLITE_LOCKED, plus the I/O error SQLite emits when a
/// concurrent creator deletes the journal/WAL file out from under us.
fn is_busy_error(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::PoolTimedOut => true,
        _ => matches!(
            sqlite_error_code(error),
            Some(SQLITE_BUSY) | Some(SQLITE_LOCKED) | Some(SQLITE_IOERR_DELETE_NOENT)
        ),
    }
}

fn migrate_error_sqlx(err: &MigrateError) -> Option<&sqlx::Error> {
    match err {
        MigrateError::Execute(e) | MigrateError::ExecuteMigration(e, _) => Some(e),
        _ => None,
    }
}

fn migrate_error_is_shmmap(err: &MigrateError) -> bool {
    migrate_error_sqlx(err).is_some_and(is_shmmap_error)
}

fn migrate_error_is_busy(err: &MigrateError) -> bool {
    migrate_error_sqlx(err).is_some_and(is_busy_error)
}

/// Held for the duration of `Database::new`. Dropping it unblocks the next waiter.
struct InitLock {
    _release: std::sync::mpsc::Sender<()>,
}

fn init_lock_path(db_path: &Path) -> PathBuf {
    let mut path = db_path.as_os_str().to_owned();
    path.push(".init.lock");
    PathBuf::from(path)
}

/// Acquire an exclusive flock on a sibling of the SQLite file.
///
/// Must not lock the `.db` itself: SQLite uses POSIX locks on that file.
/// The lock is held on a dedicated OS thread so waiting does not stall
/// the tokio runtime (a blocking flock on a worker thread deadlocks other
/// tasks that already hold the lock and are awaiting I/O).
async fn acquire_init_lock(db_path: &Path) -> CacheResult<InitLock> {
    let lock_path = init_lock_path(db_path);
    let (acquired_tx, acquired_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();

    let _ = std::thread::spawn(move || {
        let file = match OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
        {
            Ok(file) => file,
            Err(e) => {
                let _ = acquired_tx.send(Err(e));
                return;
            }
        };
        let mut lock = fd_lock::RwLock::new(file);
        let _guard = match lock.write() {
            Ok(guard) => guard,
            Err(e) => {
                let _ = acquired_tx.send(Err(e));
                return;
            }
        };
        if acquired_tx.send(Ok(())).is_err() {
            return;
        }
        let _ = release_rx.recv();
        debug!(path = %lock_path.display(), "released cache init lock");
    });

    match acquired_rx.await {
        Ok(Ok(())) => Ok(InitLock {
            _release: release_tx,
        }),
        Ok(Err(e)) => Err(CacheError::initialization(format!(
            "failed to lock {} for cache init: {e}",
            db_path.display()
        ))),
        Err(_) => Err(CacheError::initialization(format!(
            "cache init lock task ended for {}",
            db_path.display()
        ))),
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
        let migrator = create_migrator(&temp_dir).await;

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

        // Guard against the DELETE-mode fallback firing in the common case.
        let (journal_mode,): (String,) = sqlx::query_as("PRAGMA journal_mode")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(journal_mode, "wal");

        // Close database
        db.close().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn concurrent_cold_open_succeeds() {
        // Several processes creating the same missing database must all
        // succeed. This is the eval-cache / task-cache race on a cold
        // `.devenv/` (#3133).
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let migrations_dir = temp_dir.path().join("migrations");
        std::fs::create_dir_all(&migrations_dir).unwrap();
        std::fs::write(
            migrations_dir.join("20250507000001_test.sql"),
            "CREATE TABLE test (id INTEGER PRIMARY KEY, name TEXT)",
        )
        .unwrap();

        let mut handles = Vec::new();
        for _ in 0..8 {
            let path = db_path.clone();
            let dir = migrations_dir.clone();
            handles.push(tokio::spawn(async move {
                let migrator = Migrator::new(dir).await.unwrap();
                Database::new(path, &migrator).await
            }));
        }

        let mut dbs = Vec::new();
        for handle in handles {
            let db = handle
                .await
                .unwrap()
                .expect("concurrent cold Database::new should succeed");
            sqlx::query("SELECT 1").fetch_one(db.pool()).await.unwrap();
            dbs.push(db);
        }

        let (journal_mode,): (String,) = sqlx::query_as("PRAGMA journal_mode")
            .fetch_one(dbs[0].pool())
            .await
            .unwrap();
        assert_eq!(journal_mode, "wal");

        for db in dbs {
            db.close().await;
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_warm_open_succeeds() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let migrator = create_migrator(&temp_dir).await;
        let warm = Database::new(db_path.clone(), &migrator).await.unwrap();
        warm.close().await;

        let migrations_dir = temp_dir.path().join("migrations");
        let mut handles = Vec::new();
        for _ in 0..8 {
            let path = db_path.clone();
            let dir = migrations_dir.clone();
            handles.push(tokio::spawn(async move {
                let migrator = Migrator::new(dir).await.unwrap();
                Database::new(path, &migrator).await
            }));
        }
        for handle in handles {
            let db = handle
                .await
                .unwrap()
                .expect("concurrent warm Database::new should succeed");
            db.close().await;
        }
    }

    async fn create_migrator(temp_dir: &TempDir) -> Migrator {
        let migrations_dir = temp_dir.path().join("migrations");
        std::fs::create_dir_all(&migrations_dir).unwrap();
        std::fs::write(
            migrations_dir.join("20250507000001_test.sql"),
            "CREATE TABLE test (id INTEGER PRIMARY KEY, name TEXT)",
        )
        .unwrap();
        Migrator::new(migrations_dir).await.unwrap()
    }
}
