//! This module provides a SQLite-based implementation for tracking file modifications
//! related to tasks' `exec_if_modified` feature.

use devenv_cache_core::{
    db::{Database, Dir},
    error::{CacheError, CacheResult},
    file::TrackedFile,
    time,
};
use glob::{MatchOptions, glob_with};
use include_dir::include_dir;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;
use tracing::{debug, warn};

/// Expand glob patterns into actual file paths
///
/// The expansion uses strict matching rules:
/// - `case_sensitive: true` – patterns are case-sensitive (e.g. `*.RS` will not match `main.rs`)
/// - `require_literal_separator: true` – path separators must appear literally in the pattern
///   (e.g. `src**/*.rs` is invalid and will not match anything)
/// - `require_literal_leading_dot: true` – a leading dot must be written explicitly
///   (e.g. `*` will not match `.hidden`, you must use `.*` or `.hidden` directly)
pub fn expand_glob_patterns(patterns: &[String]) -> Vec<String> {
    patterns
        .iter()
        .flat_map(|pattern| {
            glob_with(
                pattern,
                MatchOptions {
                    case_sensitive: true,
                    require_literal_separator: true,
                    require_literal_leading_dot: true,
                },
            )
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|path| path.ok())
            .filter_map(|path| path.to_str().map(String::from))
        })
        .collect()
}

/// Embedded migrations directory for the task cache database.
/// These are included at compile time so they work in Nix-built binaries.
pub static MIGRATIONS_DIR: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/migrations");

/// Task cache manager
#[derive(Clone)]
pub struct TaskCache {
    db: Arc<Database>,
}

impl std::fmt::Debug for TaskCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskCache")
            .field("db_path", &self.db.path())
            .finish()
    }
}

impl TaskCache {
    /// Create a new TaskCache using the DEVENV_DOTFILE environment variable.
    pub async fn new() -> CacheResult<Self> {
        let cache_dir = std::env::var("DEVENV_DOTFILE")
            .map_err(|_| CacheError::missing_env_var("DEVENV_DOTFILE"))?;

        // Proper path joining instead of string concatenation
        let mut db_path = PathBuf::from(cache_dir);
        db_path.push("tasks.db");

        Self::with_db_path(db_path).await
    }

    /// Create a new TaskCache with a specific database path.
    pub async fn with_db_path(db_path: PathBuf) -> CacheResult<Self> {
        // Connect to database using the shared database library and run migrations
        let db = Database::new_with_embedded_migrations(db_path, &MIGRATIONS_DIR).await?;

        Ok(Self { db: Arc::new(db) })
    }

    /// Get the database reference
    pub fn db(&self) -> &Database {
        &self.db
    }

    /// Check if any files have been modified for a given task.
    ///
    /// Returns true if any of the files have been modified since the last time
    /// the task was run, or if this is the first time checking these files.
    pub async fn check_modified_files(
        &self,
        task_name: &str,
        files: &[String],
    ) -> CacheResult<bool> {
        if files.is_empty() {
            return Ok(false);
        }

        // Expand all patterns using glob and collect results
        let expanded_paths = expand_glob_patterns(files);

        // Check all files and track if any are modified
        // Important: We need to check ALL files, not return early,
        // so that all files get recorded in the database
        let mut any_modified = false;
        for path in &expanded_paths {
            let modified = self.is_file_modified(task_name, path).await?;
            if modified {
                any_modified = true;
                // Continue checking other files instead of returning early
            }
        }

        Ok(any_modified)
    }

    /// Get current Unix timestamp
    fn now() -> i64 {
        time::system_time_to_unix_seconds(SystemTime::now())
    }

    /// Store task output in the cache.
    pub async fn store_task_output(&self, task_name: &str, output: &Value) -> CacheResult<()> {
        let output_json = serde_json::to_string(output)?;
        let now = Self::now();
        let conn = self.db.connect().await?;

        conn.execute(
            r#"
            INSERT INTO task_run (task_name, last_run, output)
            VALUES (?, ?, ?)
            ON CONFLICT (task_name) DO UPDATE SET
                last_run = excluded.last_run,
                output = excluded.output
            "#,
            turso::params![task_name, now, output_json],
        )
        .await?;

        Ok(())
    }

    /// Get task output from the cache.
    pub async fn get_task_output(&self, task_name: &str) -> CacheResult<Option<Value>> {
        let conn = self.db.connect().await?;

        let mut stmt = conn
            .prepare(
                r#"
            SELECT output FROM task_run WHERE task_name = ?
            "#,
            )
            .await?;

        let mut rows = stmt.query(turso::params![task_name]).await?;

        match rows.next().await? {
            Some(row) => {
                let json_str: String = row.get(0)?;
                Ok(serde_json::from_str(&json_str)?)
            }
            None => Ok(None),
        }
    }

    /// Update the file state in the database.
    async fn update_file_state_with_file(
        &self,
        task_name: &str,
        tracked_file: &TrackedFile,
    ) -> CacheResult<()> {
        let path_str = tracked_file.path.to_str().unwrap_or("");
        let is_directory = tracked_file.is_directory;
        let content_hash = tracked_file.content_hash.clone();
        let modified_time = time::system_time_to_unix_seconds(tracked_file.modified_at);
        let conn = self.db.connect().await?;

        conn.execute(
            r#"
            INSERT INTO watched_file (task_name, path, modified_time, content_hash, is_directory)
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT (task_name, path) DO UPDATE SET
                modified_time = excluded.modified_time,
                content_hash = excluded.content_hash,
                is_directory = excluded.is_directory
            "#,
            turso::params![
                task_name,
                path_str,
                modified_time,
                content_hash,
                is_directory
            ],
        )
        .await?;

        Ok(())
    }

    /// Update the file state in the database.
    pub async fn update_file_state(&self, task_name: &str, path: &str) -> CacheResult<()> {
        debug!(
            "Updating file state for task '{}', path '{}'",
            task_name, path
        );
        // Use the TrackedFile from the shared library to compute file info
        let tracked_file = TrackedFile::new(path)?;
        self.update_file_state_with_file(task_name, &tracked_file)
            .await
    }

    /// Fetch file information from the database
    pub async fn fetch_file_info(
        &self,
        task_name: &str,
        path: &str,
    ) -> CacheResult<Option<FileInfo>> {
        let conn = self.db.connect().await?;

        let mut stmt = conn
            .prepare(
                r#"
            SELECT modified_time, content_hash, is_directory
            FROM watched_file
            WHERE task_name = ? AND path = ?
            "#,
            )
            .await?;

        let mut rows = stmt.query(turso::params![task_name, path]).await?;

        match rows.next().await? {
            Some(row) => {
                let modified_time: i64 = row.get(0)?;
                let content_hash: Option<String> = row.get(1)?;
                let is_directory: bool = row.get(2)?;
                Ok(Some(FileInfo {
                    modified_time,
                    content_hash,
                    is_directory,
                }))
            }
            None => Ok(None),
        }
    }

    /// Check if a file has been modified since the last time the task was run.
    async fn is_file_modified(&self, task_name: &str, path: &str) -> CacheResult<bool> {
        debug!(
            "Checking if file '{}' is modified for task '{}'",
            path, task_name
        );

        // Fetch the existing file info
        let file_info = self.fetch_file_info(task_name, path).await?;

        // If file not in database, consider it modified
        if file_info.is_none() {
            debug!(
                "File {} not found in cache for task {} - considering it modified (first time)",
                path, task_name
            );
            self.update_file_state(task_name, path).await?;
            return Ok(true);
        }

        // Check if file exists and get its current state
        match TrackedFile::new(path) {
            Ok(current_file) => {
                let info = file_info.unwrap();

                // Get current values
                let current_modified_time =
                    time::system_time_to_unix_seconds(current_file.modified_at);
                let current_hash = current_file.content_hash.clone();

                debug!(
                    "File '{}' for task '{}': stored_hash={:?}, current_hash={:?}, stored_time={}, current_time={}, is_dir={}",
                    path,
                    task_name,
                    info.content_hash,
                    current_hash,
                    info.modified_time,
                    current_modified_time,
                    info.is_directory
                );

                // Combine checking for file type and hash changes
                let content_changed = current_file.is_directory != info.is_directory
                    || current_hash != info.content_hash;

                if content_changed {
                    debug!(
                        "File {} changed for task {}: type or content changed (is_dir: {} -> {}, hash: {:?} -> {:?})",
                        path,
                        task_name,
                        info.is_directory,
                        current_file.is_directory,
                        info.content_hash,
                        current_hash
                    );
                    // Update the file state using the already loaded instance
                    self.update_file_state_with_file(task_name, &current_file)
                        .await?;
                    return Ok(true);
                }

                // If only timestamp changed but hash didn't, update the timestamp without considering it modified
                if current_modified_time > info.modified_time {
                    debug!(
                        "File {} timestamp changed for task {} but content is the same (time: {} -> {})",
                        path, task_name, info.modified_time, current_modified_time
                    );
                    // Update using the current file instance we already have
                    self.update_file_state_with_file(task_name, &current_file)
                        .await?;
                }

                debug!("File '{}' for task '{}' is unchanged", path, task_name);
                Ok(false)
            }
            Err(e) => {
                warn!("Failed to check file {}: {}", path, e);
                // File doesn't exist or is inaccessible, consider unchanged
                Ok(false)
            }
        }
    }
}

/// File information from the database
#[derive(Debug)]
pub struct FileInfo {
    pub modified_time: i64,
    pub content_hash: Option<String>,
    pub is_directory: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::fs::File;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn test_task_cache_initialization() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("tasks.db");

        // Use with_db_path directly instead of environment variable
        let cache = TaskCache::with_db_path(db_path).await.unwrap();

        // Check if the database connection is valid using a simple query
        let conn = cache.db().connect().await.unwrap();
        let mut stmt = conn.prepare("SELECT 1").await.unwrap();
        let result = stmt.query(()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_file_modification_detection() {
        let db_temp_dir = TempDir::new().unwrap();
        let db_path = db_temp_dir.path().join("tasks-file-mod.db");

        let cache = TaskCache::with_db_path(db_path).await.unwrap();
        let test_temp_dir = TempDir::new().unwrap();
        let file_path = test_temp_dir.path().join("test.txt");

        // Create a test file
        {
            let mut file = File::create(&file_path).await.unwrap();
            file.write_all(b"initial content").await.unwrap();
            file.sync_all().await.unwrap(); // Ensure file is fully written to disk
        }

        let task_name = "test_task";
        let path_str = file_path.to_str().unwrap().to_string();

        // First check should consider it modified (initial run)
        assert!(
            cache
                .check_modified_files(task_name, &[path_str.clone()])
                .await
                .unwrap()
        );

        // Second check should consider it unmodified
        assert!(
            !cache
                .check_modified_files(task_name, &[path_str.clone()])
                .await
                .unwrap()
        );

        // More reliable approach to ensure modification time changes:
        // 1. Sleep to ensure system time advances
        // 2. Change content to guarantee hash changes
        // 3. Use multiple write operations to maximize chance of timestamp change

        // Modify file content and set mtime to ensure detection
        {
            let mut file = File::create(&file_path).await.unwrap();
            file.write_all(b"modified content with more text")
                .await
                .unwrap();
            file.sync_all().await.unwrap(); // Force flush to filesystem

            // Set mtime to ensure it's different from original
            let new_time = std::time::SystemTime::now() + std::time::Duration::from_secs(1);
            file.into_std().await.set_modified(new_time).unwrap();
        }

        // Check should detect the modification
        assert!(
            cache
                .check_modified_files(task_name, &[path_str.clone()])
                .await
                .unwrap()
        );

        // Another check should see it as unmodified again
        assert!(
            !cache
                .check_modified_files(task_name, &[path_str])
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn test_glob_pattern_support() {
        let db_temp_dir = TempDir::new().unwrap();
        let db_path = db_temp_dir.path().join("tasks-glob.db");

        let cache = TaskCache::with_db_path(db_path).await.unwrap();
        let test_temp_dir = TempDir::new().unwrap();

        // Create test files
        let file1_path = test_temp_dir.path().join("test1.txt");
        let file2_path = test_temp_dir.path().join("test2.txt");
        let file3_path = test_temp_dir.path().join("other.log");

        {
            let mut file1 = File::create(&file1_path).await.unwrap();
            file1.write_all(b"content1").await.unwrap();
            file1.sync_all().await.unwrap();

            let mut file2 = File::create(&file2_path).await.unwrap();
            file2.write_all(b"content2").await.unwrap();
            file2.sync_all().await.unwrap();

            let mut file3 = File::create(&file3_path).await.unwrap();
            file3.write_all(b"content3").await.unwrap();
            file3.sync_all().await.unwrap();
        }

        let task_name = "test_glob_task";
        let pattern = format!("{}/*.txt", test_temp_dir.path().to_str().unwrap());

        // First check should consider files modified (initial run)
        assert!(
            cache
                .check_modified_files(task_name, &[pattern.clone()])
                .await
                .unwrap()
        );

        // Second check should consider them unmodified
        assert!(
            !cache
                .check_modified_files(task_name, &[pattern.clone()])
                .await
                .unwrap()
        );

        // Modify one of the matched files
        {
            let mut file1 = File::create(&file1_path).await.unwrap();
            file1.write_all(b"modified content1").await.unwrap();
            file1.sync_all().await.unwrap();

            let new_time = std::time::SystemTime::now() + std::time::Duration::from_secs(1);
            file1.into_std().await.set_modified(new_time).unwrap();
        }

        // Check should detect the modification
        assert!(
            cache
                .check_modified_files(task_name, &[pattern.clone()])
                .await
                .unwrap()
        );

        // Test with multiple patterns
        let pattern2 = format!("{}/*.log", test_temp_dir.path().to_str().unwrap());
        let patterns = vec![pattern.clone(), pattern2];

        // First check with new pattern should detect the .log file as modified (not tracked before)
        assert!(
            cache
                .check_modified_files(task_name, &patterns)
                .await
                .unwrap()
        );

        // Second check should consider all files unmodified
        assert!(
            !cache
                .check_modified_files(task_name, &patterns)
                .await
                .unwrap()
        );

        // Modify the .log file
        {
            let mut file3 = File::create(&file3_path).await.unwrap();
            file3.write_all(b"modified log content").await.unwrap();
            file3.sync_all().await.unwrap();

            let new_time = std::time::SystemTime::now() + std::time::Duration::from_secs(2);
            file3.into_std().await.set_modified(new_time).unwrap();
        }

        // Check should detect the modification in .log file
        assert!(
            cache
                .check_modified_files(task_name, &patterns)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn test_directory_modification_detection() {
        let db_temp_dir = TempDir::new().unwrap();
        let db_path = db_temp_dir.path().join("tasks-dir-mod.db");

        let cache = TaskCache::with_db_path(db_path).await.unwrap();
        let test_temp_dir = TempDir::new().unwrap();
        let dir_path = test_temp_dir.path().join("test_dir");
        tokio::fs::create_dir(&dir_path).await.unwrap();

        let task_name = "test_task_dir";
        let dir_path_str = dir_path.to_str().unwrap().to_string();

        // First check should consider it modified (initial run)
        assert!(
            cache
                .check_modified_files(task_name, &[dir_path_str.clone()])
                .await
                .unwrap()
        );

        // Second check should consider it unmodified
        assert!(
            !cache
                .check_modified_files(task_name, &[dir_path_str.clone()])
                .await
                .unwrap()
        );

        // Add a new file in the directory
        let file_path = dir_path.join("test_file.txt");
        {
            let mut file = File::create(&file_path).await.unwrap();
            file.write_all(b"new file content").await.unwrap();
            file.sync_all().await.unwrap();

            // Set mtime to ensure directory modification is detected
            let new_time = std::time::SystemTime::now() + std::time::Duration::from_secs(1);
            file.into_std().await.set_modified(new_time).unwrap();
        }

        // Check should detect the directory modification
        assert!(
            cache
                .check_modified_files(task_name, &[dir_path_str.clone()])
                .await
                .unwrap()
        );

        // Second check should consider it unmodified
        assert!(
            !cache
                .check_modified_files(task_name, &[dir_path_str.clone()])
                .await
                .unwrap()
        );
    }
}
